use std::convert::TryFrom;

use anyhow::{anyhow, Context, Error};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server};
use log::{debug, info};
use tokio::sync::oneshot::Receiver;

use crate::mock::model::{Expectation, HttpMethod, IncomingRequest};
use crate::mock::service::search_for_mock;
use crate::mock::service::SharedState;

use super::not_found_response;

pub struct MockServerContext {
    pub http_bind: String,
    pub state: SharedState,
    pub close_channel: Receiver<()>,
}

pub(crate) async fn server(context: MockServerContext) -> Result<(), Error> {
    let MockServerContext {
        http_bind,
        state,
        close_channel,
    } = context;

    let make_svc = make_service_fn(move |_| {
        let state = state.clone();
        async {
            // TODO add middleware for hyper server
            Ok::<_, Error>(service_fn(move |req| {
                debug!(
                    "Received http request {} on {}",
                    req.method(),
                    req.uri().path()
                );
                handler(req, state.clone())
            }))
        }
    });

    let addr = http_bind
        .parse()
        .context(format!("{} is not a valid ip config", http_bind))?;
    let server = Server::bind(&addr)
        .serve(make_svc)
        .with_graceful_shutdown(async {
            close_channel.await.ok();
        });

    info!("Http server started on http://{}", addr);
    server.await.context("Error on web server execution")
}

async fn handler<T>(req: Request<T>, state: SharedState) -> Result<Response<Body>, Error> {
    match search_for_mock(&req, state).await? {
        Some(expectation) => Response::try_from(expectation),
        None => not_found_response(),
    }
}

impl TryFrom<&Method> for HttpMethod {
    type Error = anyhow::Error;

    fn try_from(value: &Method) -> Result<Self, Self::Error> {
        match value {
            &Method::GET => Ok(HttpMethod::GET),
            &Method::POST => Ok(HttpMethod::POST),
            &Method::PUT => Ok(HttpMethod::PUT),
            &Method::DELETE => Ok(HttpMethod::DELETE),
            &Method::HEAD => Ok(HttpMethod::HEAD),
            method => Err(anyhow!("{} isn't managed as HttpMethod", method)),
        }
    }
}

impl<T> TryFrom<&Request<T>> for IncomingRequest {
    type Error = anyhow::Error;

    fn try_from(value: &Request<T>) -> Result<Self, Self::Error> {
        Ok(IncomingRequest {
            method: HttpMethod::try_from(value.method())?,
            path: value.uri().path().to_string(),
        })
    }
}

impl TryFrom<Expectation> for Response<Body> {
    type Error = anyhow::Error;

    fn try_from(value: Expectation) -> Result<Response<Body>, Error> {
        Response::builder()
            .status(value.response.status_code.unwrap_or(200))
            .body(
                value
                    .response
                    .body
                    .as_ref()
                    .map(|body| Body::from(body.clone()))
                    .unwrap_or(Body::empty()),
            )
            .context(format!("Error creating http response for {:?}", value))
    }
}
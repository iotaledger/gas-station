// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

mod access_controller;
mod endpoint_types;
mod error;

use std::io;
use std::net::Ipv4Addr;

use axum::Router;
use tokio::net::TcpListener;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

use crate::error::RequestError;

#[derive(OpenApi)]
struct ApiDoc;

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    let open_api_router =
        OpenApiRouter::with_openapi(ApiDoc::openapi()).merge(access_controller::router());

    let router: Router;
    #[cfg(feature = "swagger-ui")]
    {
        // add test endpoints and get router
        let (router_without_swagger_ui, api) = open_api_router.split_for_parts();
        router = router_without_swagger_ui.merge(
            utoipa_swagger_ui::SwaggerUi::new("/swagger-ui").url("/apidoc/openapi.json", api),
        );
    }
    #[cfg(not(feature = "swagger-ui"))]
    {
        // get router with check endpoint
        (router, _) = open_api_router.split_for_parts();
    }

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 8080)).await?;
    axum::serve(listener, router).await
}

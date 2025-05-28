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

const HOST: Ipv4Addr = Ipv4Addr::LOCALHOST;
const PORT: u16 = 8080;
const SWAGGER_UI_PATH: &str = "/swagger-ui";
const SWAGGER_FILE_PATH: &str = "/apidoc/openapi.json";

#[derive(OpenApi)]
struct ApiDoc;

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    let local_url = format!("http://{}:{}", HOST, PORT);
    let mut startup_message = format!("{: <32}{}", "hook listening on: ", &local_url);

    let open_api_router =
        OpenApiRouter::with_openapi(ApiDoc::openapi()).merge(access_controller::router());

    let router: Router;
    #[cfg(feature = "swagger-ui")]
    {
        // add test endpoints and get router
        let (router_without_swagger_ui, api) = open_api_router.split_for_parts();
        router = router_without_swagger_ui
            .merge(utoipa_swagger_ui::SwaggerUi::new(SWAGGER_UI_PATH).url(SWAGGER_FILE_PATH, api));
        startup_message.push_str(&format!(
            "\n{: <32}{}{}",
            "OpenAPI UI served on:", &local_url, SWAGGER_UI_PATH
        ));
        startup_message.push_str(&format!(
            "\n{: <32}{}{}",
            "OpenAPI API spec file on: ", &local_url, SWAGGER_FILE_PATH
        ));
    }
    #[cfg(not(feature = "swagger-ui"))]
    {
        // get router with check endpoint
        (router, _) = open_api_router.split_for_parts();
    }

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 8080)).await?;
    println!("{startup_message}");
    axum::serve(listener, router).await
}

//! The web frameworks and HTTP clients the Rust pack knows (spec #30), one entry per object, as
//! `messages.rs` lists the brokers: what makes the object outside the repository (the name the
//! `use` declarations give the call) and the methods that hand that same object back. The value
//! flow reads the methods through `LanguagePack::passes`; the HTTP kind (`http/rust/axum.rs` for
//! axum, `http/rust.rs` for actix-web and Rocket) reads what a constructor makes through `made`.

/// What an object of a framework or a client library is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Made {
    /// axum's `Router`.
    Router,
    /// axum's method router: `get(h)`, `any(h)`, `MethodRouter::new()`.
    MethodRouter,
    /// actix-web's `App`.
    App,
    /// actix-web's `Scope` (`web::scope(prefix)`).
    Scope,
    /// actix-web's `Resource` (`web::resource(path)`).
    Resource,
    /// actix-web's `ServiceConfig`, which a function receives and nothing constructs.
    Config,
    /// Rocket's instance (`rocket::build()`).
    Rocket,
    /// reqwest's `Client` and `ClientBuilder`.
    Reqwest,
    /// ureq's `Agent` and `AgentBuilder`.
    Ureq,
}

/// One object: what it is, what makes it and the methods that hand it back. A method not listed
/// (one that narrows what is served, such as `guard` or `Resource::to`, a response's `json()`)
/// makes something else, so the chain is not followed through it.
pub(crate) struct Framework {
    pub made: Made,
    pub constructors: &'static [&'static str],
    pub passes: &'static [&'static str],
}

/// What constructs a reqwest or a ureq client, an object whose methods send requests. A
/// builder chain (`Client::builder().build()?`) is the constructor that starts it.
pub(crate) const REQWEST_CLIENTS: &[&str] = &[
    "reqwest.Client.new",
    "reqwest.Client.builder",
    "reqwest.ClientBuilder.new",
    "reqwest.blocking.Client.new",
    "reqwest.blocking.Client.builder",
    "reqwest.blocking.ClientBuilder.new",
];
pub(crate) const UREQ_AGENTS: &[&str] = &[
    "ureq.agent",
    "ureq.Agent.new",
    "ureq.Agent.new_with_defaults",
    "ureq.Agent.new_with_config",
    "ureq.AgentBuilder.new",
];

/// The HTTP methods the `Method` constants of the HTTP crates name, which an actix-web
/// `web::method(Method::X)` may be given.
pub(crate) const VERBS: &[&str] = &[
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "CONNECT", "TRACE",
];

// ponytail: a setter not listed ends the chain, so what it is called on is not read; add it here.
pub(crate) const FRAMEWORKS: &[Framework] = &[
    Framework {
        made: Made::Router,
        constructors: &["axum.Router.new", "axum.Router.default"],
        passes: &[
            "clone",
            "route",
            "route_service",
            "nest",
            "nest_service",
            "merge",
            "layer",
            "route_layer",
            "with_state",
            "fallback",
            "fallback_service",
        ],
    },
    Framework {
        made: Made::MethodRouter,
        constructors: &[
            "axum.routing.any",
            "axum.routing.get",
            "axum.routing.post",
            "axum.routing.put",
            "axum.routing.patch",
            "axum.routing.delete",
            "axum.routing.head",
            "axum.routing.options",
            "axum.routing.trace",
            "axum.routing.MethodRouter.new",
        ],
        // The methods that leave the methods it serves as they are.
        passes: &["layer", "route_layer", "with_state", "handle_error"],
    },
    Framework {
        made: Made::App,
        constructors: &["actix_web.App.new", "actix_web.App.default"],
        passes: &[
            "service",
            "route",
            "configure",
            "app_data",
            "data",
            "data_factory",
            "wrap",
            "wrap_fn",
            "default_service",
            "external_resource",
        ],
    },
    Framework {
        made: Made::Scope,
        constructors: &["actix_web.web.scope"],
        passes: &[
            "service",
            "route",
            "configure",
            "app_data",
            "data",
            "wrap",
            "wrap_fn",
            "default_service",
        ],
    },
    Framework {
        made: Made::Resource,
        constructors: &["actix_web.web.resource"],
        passes: &[
            "route",
            "name",
            "app_data",
            "data",
            "wrap",
            "wrap_fn",
            "default_service",
        ],
    },
    Framework {
        made: Made::Config,
        constructors: &[],
        passes: &["service", "route", "configure", "app_data", "data"],
    },
    Framework {
        made: Made::Rocket,
        constructors: &["rocket.build", "rocket.custom"],
        passes: &["mount", "attach", "manage", "register"],
    },
    Framework {
        made: Made::Reqwest,
        constructors: REQWEST_CLIENTS,
        passes: &[
            "clone",
            "build",
            "timeout",
            "connect_timeout",
            "read_timeout",
            "user_agent",
            "default_headers",
            "redirect",
            "referer",
            "proxy",
            "no_proxy",
            "gzip",
            "brotli",
            "deflate",
            "zstd",
            "no_gzip",
            "no_brotli",
            "no_deflate",
            "no_zstd",
            "cookie_store",
            "pool_idle_timeout",
            "pool_max_idle_per_host",
            "tcp_nodelay",
            "tcp_keepalive",
            "http1_only",
            "http2_prior_knowledge",
            "https_only",
            "danger_accept_invalid_certs",
            "danger_accept_invalid_hostnames",
            "use_rustls_tls",
            "use_native_tls",
            "add_root_certificate",
            "identity",
            "min_tls_version",
            "max_tls_version",
            "connection_verbose",
            "local_address",
            "resolve",
        ],
    },
    Framework {
        made: Made::Ureq,
        constructors: UREQ_AGENTS,
        passes: &[
            "clone",
            "build",
            "timeout",
            "timeout_connect",
            "timeout_read",
            "timeout_write",
            "user_agent",
            "max_idle_connections",
            "max_idle_connections_per_host",
            "redirects",
            "proxy",
            "tls_config",
            "https_only",
            "no_delay",
            "try_proxy_from_env",
        ],
    },
];

/// What the outside constructor `origin` (`axum.Router.new`) makes, when a framework's.
pub(crate) fn made(origin: &str) -> Option<Made> {
    let mut all = FRAMEWORKS.iter();
    all.find(|f| f.constructors.contains(&origin))
        .map(|f| f.made)
}

/// The methods that hand back an object of `made`.
pub(crate) fn passes(made: Made) -> &'static [&'static str] {
    let mut all = FRAMEWORKS.iter();
    all.find(|f| f.made == made).map_or(&[], |f| f.passes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::rust::Rust;

    #[test]
    fn what_a_constructor_makes_passes_the_methods_of_that_object_alone() {
        assert_eq!(made("actix_web.App.default"), Some(Made::App));
        assert!(Rust.passes("actix_web.App.default", "service"));
        assert!(!Rust.passes("actix_web.App.default", "guard"));
        // A name of the crate that constructs nothing passes nothing.
        assert_eq!(made("actix_web.App.into_service"), None);
        assert!(!Rust.passes("actix_web.App.into_service", "service"));
        assert!(Rust.passes("reqwest.blocking.Client.builder", "timeout"));
        assert!(!Rust.passes("reqwest.get", "timeout"));
    }
}

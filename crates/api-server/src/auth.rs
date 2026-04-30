pub mod middleware;
pub mod oauth;

#[derive(Debug, Clone)]
pub struct JwtAuth {
    pub secret: String,
}

impl JwtAuth {
    pub fn new(secret: String) -> Self {
        Self { secret }
    }
}

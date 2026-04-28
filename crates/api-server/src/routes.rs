pub mod handler {
    use crate::OrchynSchema;
    use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
    use axum::extract::State;
    
    pub async fn graphql_handler(
        State(schema): State<OrchynSchema>,
        req: GraphQLRequest,
    ) -> GraphQLResponse {
        schema.execute(req.into_inner()).await.into()
    }
}

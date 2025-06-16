use mongodb::{Client, Database};

#[derive(Debug)]
pub struct AppState {
    pub db: Database,
}

impl AppState {
    pub fn new(client: Client) -> Self {
        let db = client.database("simulation_mission");  // 用你的数据库名替换
        AppState { db }
    }
}

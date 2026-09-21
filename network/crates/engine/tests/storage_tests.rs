use engine::Storage;
use sea_orm::{EntityTrait, PaginatorTrait};

#[tokio::test]
async fn opens_and_migrates_in_memory() {
    let storage = Storage::open_in_memory().await.expect("open");
    
    let count = engine::entity::contact::Entity::find()
        .count(storage.connection())
        .await
        .expect("query");
        
    assert_eq!(count, 0);
}

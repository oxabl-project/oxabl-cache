//! The Postgres launcher yields a cluster that accepts connections and isolates databases.

use oxabl_cache_e2e::postgres::Postgres;

#[tokio::test]
async fn accepts_a_connection_and_answers_a_query() {
    let database = Postgres::shared().create_database().await.unwrap();
    let client = database.connect().await.unwrap();

    let row = client.query_one("SELECT 40 + 2", &[]).await.unwrap();

    assert_eq!(row.get::<_, i32>(0), 42);
}

#[tokio::test]
async fn databases_do_not_see_each_others_tables() {
    let first = Postgres::shared().create_database().await.unwrap();
    let second = Postgres::shared().create_database().await.unwrap();

    first
        .connect()
        .await
        .unwrap()
        .batch_execute("CREATE TABLE only_here (id int)")
        .await
        .unwrap();

    let missing = second
        .connect()
        .await
        .unwrap()
        .query("SELECT * FROM only_here", &[])
        .await;
    assert!(missing.is_err());
}

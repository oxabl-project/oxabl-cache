//! The versitygw launcher yields a working S3 endpoint with isolated buckets.

use oxabl_cache_e2e::versitygw::Versitygw;

#[tokio::test]
async fn presigned_put_then_ranged_get_returns_the_range() {
    let bucket = Versitygw::shared().new_bucket().await.unwrap();
    let body: Vec<u8> = (0..=255u8).cycle().take(4096).collect();

    bucket.put("pack/one", body.clone()).await.unwrap();

    assert_eq!(bucket.head("pack/one").await.unwrap(), Some(4096));
    assert_eq!(
        bucket.get_range("pack/one", 100, 355).await.unwrap(),
        body[100..=355]
    );
    bucket.delete("pack/one").await.unwrap();
    assert_eq!(bucket.head("pack/one").await.unwrap(), None);
}

#[tokio::test]
async fn buckets_do_not_see_each_others_objects() {
    let first = Versitygw::shared().new_bucket().await.unwrap();
    let second = Versitygw::shared().new_bucket().await.unwrap();

    first.put("shared-name", b"first".to_vec()).await.unwrap();

    assert_eq!(second.head("shared-name").await.unwrap(), None);
}

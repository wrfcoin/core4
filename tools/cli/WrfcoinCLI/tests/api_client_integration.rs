use mockito::{Matcher, Server};
use std::time::Duration;
use wrfcoin_cli::{ApiClient, ApiConfig, CliErrorCode};

#[tokio::test(flavor = "multi_thread")]
async fn get_blockchain_status_hits_api() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    let _network_mock = server
        .mock("GET", "/api/network/info")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "success": true,
            "data": {
                "chainId": 1024,
                "networkName": "mocknet",
                "consensusType": "1.0-mock"
            }
        }"#,
        )
        .create_async()
        .await;
    let _stats_mock = server
        .mock("GET", "/api/stats")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "success": true,
            "data": {
                "totalTransactions": 12345,
                "totalNodes": 12
            }
        }"#,
        )
        .create_async()
        .await;

    let config = ApiConfig {
        base_url: format!("{}/api", server.url()),
        timeout: Duration::from_secs(1),
        retry_attempts: 2,
        retry_delay: Duration::from_millis(1),
        explain_mode: false,
        bearer_token: None,
    };
    let client = ApiClient::new(config)?;

    let stats = client.get_blockchain_status().await?;
    assert_eq!(stats.network, "mocknet");
    assert_eq!(stats.version, "1.0-mock");
    assert_eq!(stats.block_height, 1024);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn get_latest_blocks_returns_expected_items() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    let _blocks_mock = server
        .mock("GET", "/api/chain/blocks")
        .match_query(Matcher::AllOf(vec![Matcher::UrlEncoded(
            "limit".into(),
            "5".into(),
        )]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "success": true,
                "data": [
                    { "hash": "abc", "height": 1, "timestamp": "2024-01-01T00:00:00Z", "prev_hash": "000", "merkle_root": "root", "size": 1, "nonce": 0, "difficulty": 1, "transactions_count": 0, "block_type": "regular" }
                ]
            }"#,
        )
        .create_async()
        .await;

    let config = ApiConfig {
        base_url: format!("{}/api", server.url()),
        timeout: Duration::from_secs(1),
        retry_attempts: 1,
        retry_delay: Duration::from_millis(1),
        explain_mode: false,
        bearer_token: None,
    };
    let client = ApiClient::new(config)?;

    let blocks = client.get_latest_blocks(5).await?;
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].hash, "abc");

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn api_error_identity_comes_from_status_not_response_prose() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    let cases = [
        (401, "/auth", CliErrorCode::AuthError),
        (404, "/missing", CliErrorCode::NotFound),
        (429, "/busy", CliErrorCode::RateLimitError),
        (422, "/invalid", CliErrorCode::ValidationError),
    ];
    let mut mocks = Vec::new();
    for (status, path, _) in cases {
        mocks.push(
            server
                .mock("GET", path)
                .with_status(status)
                .with_header("content-type", "application/json")
                .with_body(
                    r#"{"error":"not a configuration error: invalid network; resource not found"}"#,
                )
                .create_async()
                .await,
        );
    }

    let client = ApiClient::new(ApiConfig {
        base_url: server.url(),
        timeout: Duration::from_secs(1),
        retry_attempts: 0,
        retry_delay: Duration::from_millis(1),
        explain_mode: false,
        bearer_token: None,
    })?;

    for (_, path, expected) in cases {
        let error = client
            .get::<serde_json::Value>(path, None)
            .await
            .expect_err("non-success status must be returned as an error");
        assert_eq!(CliErrorCode::from_error(&error), expected);
    }
    drop(mocks);
    Ok(())
}

// ---------------------------------------------------------------------------
// Bearer token propagation (#2707)
//
// `wrfcoin oracle submit-registrations` returned 401 against every node with
// WRFCOIN_OPERATOR_TOKEN set because the shared client never sent an
// Authorization header. These pin the contract for every route the client
// serves: a configured token is sent as `Bearer <token>`, and no header at
// all is sent when none is configured (a node must see "missing", not an
// empty credential it would reject as malformed).
// ---------------------------------------------------------------------------

fn short_retry_config(base_url: String, bearer_token: Option<String>) -> ApiConfig {
    ApiConfig {
        base_url,
        timeout: Duration::from_secs(1),
        retry_attempts: 0,
        retry_delay: Duration::from_millis(1),
        explain_mode: false,
        bearer_token,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn wallet_balance_reads_chain_address_route() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/addresses/0xabc")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"data":{"address":"0xabc","balance":"42","spendable_balance":"40","stake_locked":"2","earned_balance_total":"7","nonce":"9"}}"#)
        .expect(1)
        .create_async()
        .await;
    let client = ApiClient::new(short_retry_config(server.url(), None))?;
    let account = client.get_address("0xabc").await?;
    assert_eq!(account.balance, "42");
    assert_eq!(account.nonce.as_deref(), Some("9"));
    mock.assert_async().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn wallet_send_posts_exactly_once_to_node_submit_route() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/transactions/submit")
        .match_body(Matcher::PartialJson(serde_json::json!({
            "from": "0x1111111111111111111111111111111111111111",
            "to": "0x2222222222222222222222222222222222222222",
            "amount": 12,
            "nonce": "7",
            "transparent_opt_out": true,
            "transparent_opt_out_reason": "cli",
            "auth_pubkey": "abcd",
            "auth_signature": "1234"
        })))
        .with_status(202)
        .with_header("content-type", "application/json")
        .with_body(r#"{"data":{"accepted":true,"tx_id":"0xfeed"}}"#)
        .expect(1)
        .create_async()
        .await;
    let client = ApiClient::new(short_retry_config(server.url(), None))?;
    let accepted = client
        .submit_transparent_transfer(
            "0x1111111111111111111111111111111111111111",
            "0x2222222222222222222222222222222222222222",
            12,
            7,
            "abcd",
            "1234",
        )
        .await?;
    assert!(accepted.accepted);
    assert_eq!(accepted.tx_id, "0xfeed");
    mock.assert_async().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn post_sends_bearer_header_when_token_configured() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    // The mock only matches when the exact header is present; a request
    // without it falls through to mockito's 501 and the assertion fails.
    let mock = server
        .mock("POST", "/transactions/oracle/registrations")
        .match_header("authorization", "Bearer operator-token-for-test")
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true,"data":{"accepted":3}}"#)
        .expect(1)
        .create_async()
        .await;

    let client = ApiClient::new(short_retry_config(
        server.url(),
        Some("operator-token-for-test".to_string()),
    ))?;
    let response: serde_json::Value = client
        .post(
            "/transactions/oracle/registrations",
            Some(serde_json::json!({"transactions": []})),
        )
        .await?;

    assert_eq!(response["accepted"], 3);
    mock.assert_async().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn get_sends_bearer_header_when_token_configured() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/bridge/safety/status")
        .match_header("authorization", "Bearer reader-token-for-test")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true,"data":{"halted":false}}"#)
        .expect(1)
        .create_async()
        .await;

    let client = ApiClient::new(short_retry_config(
        server.url(),
        Some("reader-token-for-test".to_string()),
    ))?;
    let response: serde_json::Value = client.get("/bridge/safety/status", None).await?;

    assert_eq!(response["halted"], false);
    mock.assert_async().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn request_omits_authorization_header_when_no_token() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/transactions/oracle/registrations")
        .match_header("authorization", Matcher::Missing)
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true,"data":{"accepted":0}}"#)
        .expect(1)
        .create_async()
        .await;

    let client = ApiClient::new(short_retry_config(server.url(), None))?;
    let response: serde_json::Value = client
        .post(
            "/transactions/oracle/registrations",
            Some(serde_json::json!({"transactions": []})),
        )
        .await?;

    assert_eq!(response["accepted"], 0);
    mock.assert_async().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn unauthenticated_401_maps_to_auth_error() -> anyhow::Result<()> {
    // The failure mode measured in #2707, as the node reports it.
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("POST", "/transactions/oracle/registrations")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"ok":false,"error":{"code":"UNAUTHORIZED","message":"Missing or malformed Authorization header"}}"#,
        )
        .create_async()
        .await;

    let client = ApiClient::new(short_retry_config(server.url(), None))?;
    let err = client
        .post::<serde_json::Value>(
            "/transactions/oracle/registrations",
            Some(serde_json::json!({})),
        )
        .await
        .expect_err("401 must surface as an error");

    assert_eq!(CliErrorCode::from_error(&err), CliErrorCode::AuthError);
    Ok(())
}

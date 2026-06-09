use axum::body::to_bytes;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use httpx::{Problem, PROBLEM_CONTENT_TYPE};

async fn body_json(problem: Problem) -> (StatusCode, String, serde_json::Value) {
    let resp = problem.into_response();
    let status = resp.status();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap();
    (status, content_type, json)
}

#[tokio::test]
async fn renders_defaults_as_problem_json() {
    let (status, content_type, json) =
        body_json(Problem::new(StatusCode::NOT_FOUND, "task 7 does not exist")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(content_type, PROBLEM_CONTENT_TYPE);
    assert_eq!(json["type"], "about:blank");
    assert_eq!(json["title"], "Not Found");
    assert_eq!(json["status"], 404);
    assert_eq!(json["detail"], "task 7 does not exist");
    assert!(json.get("instance").is_none());
}

#[tokio::test]
async fn includes_extensions_and_overrides() {
    let problem = Problem::new(StatusCode::CONFLICT, "already archived")
        .with_type("https://errors.example/conflict")
        .with_title("Conflict")
        .with_instance("/tasks/7")
        .with_extension("code", "task_archived");

    let (status, _ct, json) = body_json(problem).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json["type"], "https://errors.example/conflict");
    assert_eq!(json["instance"], "/tasks/7");
    assert_eq!(json["code"], "task_archived");
}

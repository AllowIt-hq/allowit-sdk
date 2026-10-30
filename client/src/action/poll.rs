//! Status checks and bounded polling of pending policy requests.
use crate::{
    client::Client,
    error::{Error, Result},
    output::check_result,
    request::encode,
    skill::{Network, text},
};
use serde_json::{Value, json};

pub(crate) fn check_status(r: &Value, id: &str, net: Network) -> Result<()> {
    check_result(r, "status", net)?;
    let got = text(&r["requestId"]);
    if got.is_empty() {
        return Err(Error::uncertain(
            "Allowit returned a status without its requestId",
        ));
    }
    if got != id && text(&r["clientRequestId"]) != id {
        return Err(Error::uncertain(format!(
            "Allowit returned the status of a different request ({got})"
        )));
    }
    Ok(())
}
pub(crate) fn poll(
    c: &mut Client,
    id: &str,
    wait: std::time::Duration,
    r: &Value,
    command: &str,
    net: Network,
) -> Result<Value> {
    let start = text(&r["status"]);
    let mut r = r.clone();
    let mut elapsed = std::time::Duration::ZERO;
    while elapsed < wait {
        std::thread::sleep(std::time::Duration::from_secs(2));
        let next = c
            .call("POST", "status", Some(&encode(&json!({"requestId":id}))))
            .map_err(|e| Error::uncertain(format!("checking the request status failed: {e}")))?;
        check_status(&next, id, net)?;
        if command != "status" {
            check_result(&next, command, net)?;
        }
        r = next;
        if text(&r["status"]) != start {
            break;
        }
        elapsed += std::time::Duration::from_secs(2);
    }
    Ok(r)
}

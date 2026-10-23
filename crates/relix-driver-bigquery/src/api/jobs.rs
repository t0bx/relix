/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::client::BigQueryClient;
use crate::api::models::{JobReference, QueryRequest, QueryResults};
use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::RunningJob;

pub(crate) const WAIT_MS: u64 = 2_000;

/// Runs a query with `jobs.query` and long-polls `jobs.getQueryResults` until the job is
/// complete. Every wait is capped at two seconds, so a cancel requested while the job id
/// was still unknown is delivered (`jobs.cancel`) at most that late; once known, the
/// tracker lets `cancel` reach the job directly.
pub(crate) async fn run_query(
    client: &BigQueryClient,
    project: &str,
    request: &QueryRequest,
    job: Option<&RunningJob<JobReference>>,
) -> Result<QueryResults, DriverError> {
    let statement = Some(request.query.as_str());
    let mut results: QueryResults = client
        .post(
            &["projects", project, "queries"],
            &[],
            Some(request),
            statement,
        )
        .await?;
    let session_info = results.session_info.clone();
    let mut attached = false;
    while !results.job_complete.unwrap_or(false) {
        let reference = results.job_reference.clone().ok_or_else(|| {
            DriverError::new(
                ErrorKind::Protocol,
                "BigQuery reported an incomplete query without a job reference",
            )
        })?;
        if let Some(job) = job.filter(|_| !attached) {
            attached = true;
            if job.attach(reference.clone()) {
                let _ = cancel_job(client, &reference).await;
            }
        }
        results = query_results(client, &reference, None, request.max_results, statement).await?;
    }
    if results.session_info.is_none() {
        results.session_info = session_info;
    }
    Ok(results)
}

pub(crate) async fn query_results(
    client: &BigQueryClient,
    reference: &JobReference,
    page_token: Option<&str>,
    max_results: u32,
    statement: Option<&str>,
) -> Result<QueryResults, DriverError> {
    let mut query = vec![
        ("maxResults", max_results.to_string()),
        ("timeoutMs", WAIT_MS.to_string()),
        ("formatOptions.useInt64Timestamp", "true".to_owned()),
    ];
    if let Some(location) = &reference.location {
        query.push(("location", location.clone()));
    }
    if let Some(token) = page_token {
        query.push(("pageToken", token.to_owned()));
    }
    client
        .get(
            &[
                "projects",
                &reference.project_id,
                "queries",
                &reference.job_id,
            ],
            &query,
            statement,
        )
        .await
}

pub(crate) async fn cancel_job(
    client: &BigQueryClient,
    reference: &JobReference,
) -> Result<(), DriverError> {
    let query: Vec<(&str, String)> = reference
        .location
        .iter()
        .map(|location| ("location", location.clone()))
        .collect();
    client
        .post::<serde_json::Value, ()>(
            &[
                "projects",
                &reference.project_id,
                "jobs",
                &reference.job_id,
                "cancel",
            ],
            &query,
            None,
            None,
        )
        .await
        .map(|_| ())
}

use affinidi_messaging_didcomm::UnpackMetadata;
use affinidi_messaging_sdk::messages::{
    DeleteMessageRequest, DeleteMessageResponse, GenericDataStruct,
};
use axum::{extract::State, Json};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{debug, span, warn, Instrument, Level};

use crate::{
    common::errors::{AppError, MediatorError, Session, SuccessResponse},
    SharedData,
};

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ResponseData {
    pub body: String,
    pub metadata: UnpackMetadata,
}
impl GenericDataStruct for ResponseData {}

/// Deletes a specific message from ATM
/// Returns a list of messages that were deleted
pub async fn message_delete_handler(
    session: Session,
    State(state): State<SharedData>,
    Json(body): Json<DeleteMessageRequest>,
) -> Result<(StatusCode, Json<SuccessResponse<DeleteMessageResponse>>), AppError> {
    let _span = span!(
        Level::DEBUG,
        "message_delete_handler",
        session = session.session_id,
        did = session.did,
    );
    async move {
        debug!("Deleting ({}) messages", body.message_ids.len());
        if body.message_ids.len() > state.config.max_deleted_messages.try_into().unwrap() {
            return Err(MediatorError::RequestDataError(
                session.session_id.clone(),
                format!(
                    "Operation exceeds the allowed limit. You may delete a maximum of 100 messages per request. Received {} ids.",
                    body.message_ids.len()
                ),
            )
            .into());
        }
        let mut deleted: DeleteMessageResponse = DeleteMessageResponse::default();

        for message in &body.message_ids {
            debug!("Deleting message: message_id({})", message);
            let result = state
                .database
                .delete_message(&session.session_id, &session.did_hash, message)
                .await;

            match result {
                Ok(_) => deleted.success.push(message.into()),
                Err(err) => {
                    warn!("failed to delete msg({}). Reason: {}", message, err);
                    deleted.errors.push((message.into(), err.to_string()));
                }
            }
        }

        Ok((
            StatusCode::OK,
            Json(SuccessResponse {
                sessionId: session.session_id,
                httpCode: StatusCode::OK.as_u16(),
                errorCode: 0,
                errorCodeStr: "NA".to_string(),
                message: "Success".to_string(),
                data: Some(deleted),
            }),
        ))
    }
    .instrument(_span)
    .await
}

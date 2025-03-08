use crate::{
    common::errors::{MediatorError, Session},
    database::DatabaseHandler,
    messages::{MessageHandler, MessageResponse, PackOptions, ProcessMessageResponse},
    SharedData,
};
use affinidi_messaging_didcomm::{envelope::MetaEnvelope, Message, UnpackOptions};
use affinidi_messaging_sdk::messages::sending::{InboundMessageList, InboundMessageResponse};
use futures::future::try_join_all;
use sha256::digest;
use tracing::{debug, error, span, trace, warn, Instrument};
pub(crate) async fn handle_inbound(
    state: &SharedData,
    session: &Session,
    message: &str,
) -> Result<InboundMessageResponse, MediatorError> {
    let _span = span!(tracing::Level::DEBUG, "handle_inbound",);

    async move {
        let mut envelope =
            match MetaEnvelope::new(message, &state.did_resolver, &state.config.mediator_secrets)
                .await
            {
                Ok(envelope) => envelope,
                Err(e) => {
                    return Err(MediatorError::ParseError(
                        session.session_id.clone(),
                        "Raw inbound DIDComm message".into(),
                        e.to_string(),
                    ));
                }
            };
        debug!("message converted to MetaEnvelope");

        // Unpack the message
        let (msg, metadata) = match Message::unpack(
            &mut envelope,
            &state.did_resolver,
            &state.config.mediator_secrets,
            &UnpackOptions {
                crypto_operations_limit_per_message: state
                    .config
                    .crypto_operations_per_message_limit,
                ..UnpackOptions::default()
            },
        )
        .await
        {
            Ok(ok) => ok,
            Err(e) => {
                return Err(MediatorError::MessageUnpackError(
                    session.session_id.clone(),
                    format!("Couldn't unpack incoming message. Reason: {}", e),
                ));
            }
        };

        debug!("message unpacked:\n{:#?}", msg);

        // Process the message

        if let Some(ProcessMessageResponse {
            message_response,
            store_message,
            force_live_delivery,
        }) = msg.process(state, session).await? {
            debug!("message processed:\n message: {message:?}\n store_message: {store_message:?}\n force_live_delivery: {force_live_delivery:?}\n");


            let to_did_packed = match message_response {
                MessageResponse::PackedMessage{ref to, packed_message} => {
                    vec![(to, packed_message)]
                },
                MessageResponse::Message(ref message @ Message {
                    to: Some(ref to_dids),
                    ..
                }) => {
                    if to_dids.len() > state.config.to_recipients_limit {
                        return Err(MediatorError::MessagePackError(
                            session.session_id.clone(),
                            format!("Recipient count({}) exceeds limit", to_dids.len()),
                        ));
                    }
                    if to_dids.is_empty() {
                        return Err(MediatorError::MessagePackError(
                            session.session_id.clone(),
                            "Recipients are empty".to_string()
                        ));
                    }

                    try_join_all(to_dids.iter().map(async |to_did| {
                        let (packed, _msg_metadata) = message
                            .pack(
                                to_did,
                                &state.config.mediator_did, // take `from` of message?
                                &metadata,
                                &state.config.mediator_secrets,
                                &state.did_resolver,
                                &PackOptions {
                                    to_keys_per_recipient_limit: state.config.to_keys_per_recipient_limit,
                                },
                            )
                            .await?;
                        Ok((to_did, packed))})).await?
                },
                _ => {
                    return Err(MediatorError::MessagePackError(
                        session.session_id.clone(),
                        format!("MessageResponse is unmatched: {message_response:?}")));
                },
            };

            for (to_did, packed) in to_did_packed.iter() {
                let to_did_hash = digest(*to_did);
                _try_live_stream(
                    state,
                    &to_did_hash,
                    packed,
                    force_live_delivery,
                )
                    .await;
            }

            if store_message {
                let mut stored_messages = InboundMessageList::default();
                for (to_did, packed) in to_did_packed {
                    match state
                        .database
                        .store_message(
                    &session.session_id,
                    &packed,
                    to_did,
                    Some(&state.config.mediator_did),
                )
                        .await
                    {
                        Ok(msg_id) => {
                            debug!(
                                "message {} stored successfully, recipient({})",
                                msg_id, to_did
                            );
                            stored_messages.messages.push((to_did.to_owned(), msg_id));
                        }
                        Err(e) => {
                            warn!("error storing message recipient({}): {:?}", to_did, e);
                            stored_messages
                                .errors
                                .push((to_did.to_owned(), e.to_string()));
                        }
                    }
                };
                Ok(InboundMessageResponse::Stored(stored_messages))
            } else {
                Ok(InboundMessageResponse::Ephemeral(to_did_packed[0].1.to_owned()))
            }
        } else {
            error!("No message to return");
            Err(MediatorError::InternalError(
                session.session_id.clone(),
                "Expected a message to return, but got None".into(),
            ))
        }
    }
    .instrument(_span)
    .await
}

/// If live streaming is enabled, this function will send the message to the live stream
/// Ok to ignore errors here
async fn _live_stream(
    database: &DatabaseHandler,
    did_hash: &str,
    stream_uuid: &str,
    packed: &str,
    force_live_delivery: bool,
) {
    if database
        .streaming_publish_message(did_hash, stream_uuid, packed, force_live_delivery)
        .await
        .is_ok()
    {
        debug!("Live streaming message to UUID: {}", stream_uuid);
    }
}

async fn _try_live_stream(
    state: &SharedData,
    did_hash: &str,
    packed: &str,
    force_live_delivery: bool,
) {
    if let Some(stream_uuid) = state
        .database
        .streaming_is_client_live(did_hash, force_live_delivery)
        .await
    {
        _live_stream(
            &state.database,
            did_hash,
            &stream_uuid,
            packed,
            force_live_delivery,
        )
        .await;
    }
}

async fn _try_store(
    state: &SharedData,
    session: &Session,
    to_did: &str,
    packed: &str,
) -> InboundMessageList {
    let mut stored_messages = InboundMessageList::default();
    match state
        .database
        .store_message(
            &session.session_id,
            packed,
            to_did,
            Some(&state.config.mediator_did),
        )
        .await
    {
        Ok(msg_id) => {
            debug!(
                "message {} stored successfully, recipient({})",
                msg_id, to_did
            );
            stored_messages.messages.push((to_did.to_owned(), msg_id));
        }
        Err(e) => {
            warn!("error storing message recipient({}): {:?}", to_did, e);
            stored_messages
                .errors
                .push((to_did.to_owned(), e.to_string()));
        }
    }
    stored_messages
}

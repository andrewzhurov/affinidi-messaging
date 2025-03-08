use crate::{
    errors::ATMError,
    messages::{GenericDataStruct, SuccessResponse},
    transports::SendMessageResponse,
    utils::Debuggable,
    ATM,
};
use affinidi_messaging_didcomm::{Attachment, AttachmentBuilder, AttachmentData, Message};
use rkyv::api::high::HighSerializer;
use rkyv::rancor::{Error, Fallible, Source};
use rkyv::ser::allocator::ArenaHandle;
use rkyv::ser::Allocator;
use rkyv::util::AlignedVec;
use rkyv::{api::high::to_bytes_with_alloc, ser::allocator::Arena};
use rkyv::{Archive, Deserialize, Serialize};
use tracing::{debug, span, Level};
use uuid::Uuid;

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[rkyv(
    // This will generate a PartialEq impl between our unarchived
    // and archived types
    compare(PartialEq),
    // Derives can be passed through to the generated type:
    derive(Debug),
)]
pub enum GRIDMessage {
    Inc,
}

impl<'c> ATM<'c> {
    /// send_didcomm_message
    /// - msg: Packed DIDComm message that we want to send
    /// - return_response: Whether to return the response from the API
    pub async fn send_didcomm_message<T>(
        &mut self,
        message: &str,
        return_response: bool,
    ) -> Result<SendMessageResponse<T>, ATMError>
    where
        T: GenericDataStruct,
    {
        // let body = std::collections::HashMap::from([("key".to_string(), "val".to_string())]);
        // debug!("example reqwest");
        // let resp = reqwest::Client::new()
        //     .post("http://httpbin.org/post")
        //     .json(&body)
        //     .send()
        //     .await
        //     .unwrap();
        // debug!("example resp {resp:#?}");

        let _span = span!(Level::DEBUG, "send_message",).entered();
        let tokens = self.authenticate().await?;
        let msg = message.to_owned();

        debug!("request /authenticate");
        let res = self
            .client
            .post(format!("{}/authenticate", self.config.atm_api))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", tokens.access_token))
            .body(msg)
            .send()
            .await
            .unwrap();

        let status = res.status();
        debug!("response /authenticate:\n {res:?}");

        let body = res
            .text()
            .await
            .map_err(|e| ATMError::TransportError(format!("Couldn't get body: {:?}", e)))?;

        if !status.is_success() {
            return Err(ATMError::TransportError(format!(
                "API returned an error: status({}), body({})",
                status, body
            )));
        }

        let http_response: Option<T> = if return_response {
            let r: SuccessResponse<T> = serde_json::from_str(&body).map_err(|e| {
                ATMError::TransportError(format!("Couldn't parse response: {:?}", e))
            })?;
            r.data
        } else {
            None
        };

        Ok(SendMessageResponse::RestAPI(http_response))
    }

    pub async fn send_rkyv_message<R>(
        &mut self,
        to: &str,
        rkyv_data: &R,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        R: for<'a> Serialize<HighSerializer<AlignedVec, ArenaHandle<'a>, Error>>
            + std::fmt::Debug
            + PartialEq,
    {
        let _span = span!(Level::DEBUG, "send",).entered();
        debug!("send({to}, {rkyv_data:?})");

        let rkyv_data_av = rkyv::to_bytes::<Error>(rkyv_data).unwrap();
        let rkyv_data_bytes = rkyv_data_av.to_vec();
        // let parsed_rkyv_data = rkyv::from_bytes::<R, Error>(&rkyv_data_bytes)?;
        // assert_eq!(rkyv_data, parsed_rkyv_data);

        let msg = Message::build(
            Uuid::new_v4().into(),
            "https://didcomm.org/rkyv/1.0/message".to_owned(),
            serde_json::Value::Null,
        )
        .to(to.to_string())
        .attachment(Attachment::rkyv(rkyv_data_bytes).finalize())
        .finalize();

        let (env, env_meta) = self
            .pack_encrypted(&msg, to, Some(&self.my_did()?), None)
            .await?;
        println!("env_meta: {env_meta:?}");

        let to_endpoint = env_meta
            .messaging_service
            .ok_or(ATMError::TransportError("No messaging service".to_string()))?
            .service_endpoint;

        let tokens = self.authenticate().await?;
        debug!("Sending grid message to endpoint: {to_endpoint}");
        let res = self
            .client
            .post(&to_endpoint)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", tokens.access_token))
            .body(env)
            .send()
            .await?;
        debug!("API response: {res:?}");

        if !res.status().is_success() {
            return Err(Box::new(ATMError::TransportError(
                "API returned an error".to_string(),
            )));
        }

        Ok(())
    }

    pub async fn send_grid_message(
        &mut self,
        protocol: &str,
        to: &str,
        grid_message: GRIDMessage,
        message: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _span = span!(Level::DEBUG, "send",).entered();
        debug!("send({to}, {message})");
        // let body = std::collections::HashMap::from([("key".to_string(), "val".to_string())]);
        // debug!("example reqwest");
        // let resp = reqwest::Client::new()
        //     .post("http://httpbin.org/post")
        //     .json(&body)
        //     .send()
        //     .await
        //     .unwrap();
        // debug!("example resp {resp:#?}");

        // let mut arena = Arena::new();
        // let bytes = to_bytes_with_alloc::<_, Error>(content, arena.acquire());

        let grid_message_av = rkyv::to_bytes::<Error>(&grid_message).unwrap();
        let grid_message_bytes = grid_message_av.to_vec();
        let parsed_grid_message = rkyv::from_bytes::<GRIDMessage, Error>(&grid_message_bytes)?;
        assert_eq!(grid_message, parsed_grid_message);

        let msg = Message::build(
            Uuid::new_v4().into(),
            protocol.to_owned(),
            serde_json::Value::Null,
        )
        .to(to.to_string())
        .attachment(Attachment::rkyv(grid_message_bytes).finalize())
        .finalize();

        let (env, env_meta) = self
            .pack_encrypted(&msg, to, Some(&self.my_did()?), None)
            .await?;
        println!("env_meta: {env_meta:?}");

        let to_endpoint = env_meta
            .messaging_service
            .ok_or(ATMError::TransportError("No messaging service".to_string()))?
            .service_endpoint;

        debug!("Sending grid message to endpoint: {to_endpoint}");
        let res = self.client.post(&to_endpoint).body(env).send().await?;
        debug!("API response: {res:?}");

        if !res.status().is_success() {
            return Err(Box::new(ATMError::TransportError(
                "API returned an error".to_string(),
            )));
        }

        Ok(())
    }
}

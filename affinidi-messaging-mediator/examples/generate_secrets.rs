use std::fs::File;
use std::io::Write;

use base64::prelude::{Engine as _, BASE64_URL_SAFE_NO_PAD};

use did_peer::{
    DIDPeer, DIDPeerCreateKeys, DIDPeerKeys, DIDPeerService, PeerServiceEndPoint,
    PeerServiceEndPointLong,
};
use ring::signature::Ed25519KeyPair;
use serde_json::json;
use ssi::{
    dids::DIDKey,
    jwk::{Params, JWK},
};

struct LocalDidPeerKeys {
    v_d: Option<String>,
    v_x: Option<String>,
    e_d: Option<String>,
    e_x: Option<String>,
    e_y: Option<String>,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Generate keys for encryption and verification
    let v_ed25519_key = JWK::generate_ed25519().unwrap();

    let e_secp256k1_key = JWK::generate_secp256k1();

    let mut local_did_peer_keys = LocalDidPeerKeys {
        v_d: None,
        v_x: None,
        e_d: None,
        e_x: None,
        e_y: None,
    };

    // Print the private keys in case you want to save them for later
    println!("Private keys:");
    if let Params::OKP(map) = v_ed25519_key.clone().params {
        println!(
            "V: private-key (d): {} {}",
            map.curve,
            String::from(map.private_key.clone().unwrap())
        );

        println!(
            "V: public-key (x): {} {}",
            map.curve,
            String::from(map.public_key.clone())
        );

        local_did_peer_keys.v_d = Some(String::from(map.private_key.clone().unwrap()));
        local_did_peer_keys.v_x = Some(String::from(map.public_key.clone()));
    }
    println!();

    if let Params::EC(map) = e_secp256k1_key.clone().params {
        println!(
            "E: private-key: {} {}",
            map.curve.clone().unwrap(),
            String::from(map.ecc_private_key.clone().unwrap())
        );
        println!(
            "E: public-key (x): {} {}",
            map.curve.clone().unwrap(),
            String::from(map.x_coordinate.clone().unwrap())
        );
        println!(
            "E: public-key (y): {} {}",
            map.curve.clone().unwrap(),
            String::from(map.y_coordinate.clone().unwrap())
        );

        local_did_peer_keys.e_d = Some(String::from(map.ecc_private_key.clone().unwrap()));
        local_did_peer_keys.e_x = Some(String::from(map.x_coordinate.clone().unwrap()));
        local_did_peer_keys.e_y = Some(String::from(map.y_coordinate.clone().unwrap()));
    }
    println!();

    // Create the did:key DID's for each key above
    let v_did_key = DIDKey::generate(&v_ed25519_key).unwrap();
    let e_did_key = DIDKey::generate(&e_secp256k1_key).unwrap();

    // Put these keys in order and specify the type of each key (we strip the did:key: from the front)
    let keys = vec![
        DIDPeerCreateKeys {
            purpose: DIDPeerKeys::Verification,
            type_: None,
            public_key_multibase: Some(v_did_key[8..].to_string()),
        },
        DIDPeerCreateKeys {
            purpose: DIDPeerKeys::Encryption,
            type_: None,
            public_key_multibase: Some(e_did_key[8..].to_string()),
        },
    ];

    // Create a service definition
    let services = vec![DIDPeerService {
        id: None,
        _type: "dm".into(),
        service_end_point: PeerServiceEndPoint::Long(PeerServiceEndPointLong {
            uri: "https://localhost:7037/mediator/v1/inboud".into(),
            accept: vec!["didcomm/v2".into()],
            routing_keys: vec![],
        }),
    }];

    // Create the did:peer DID
    let (did_peer, _) =
        DIDPeer::create_peer_did(&keys, Some(&services)).expect("Failed to create did:peer");

    println!("did = {}", did_peer);

    let secrets_json = json!([
      {
          "id": format!("{}#key-1", did_peer),
          "type": "JsonWebKey2020",
          "privateKeyJwk": {
              "crv": "Ed25519",
              "d":  local_did_peer_keys.v_d,
              "kty": "OKP",
              "x": local_did_peer_keys.v_x
          }
      },
      {
          "id": format!("{}#key-2", did_peer),
          "type": "JsonWebKey2020",
          "privateKeyJwk": {
              "crv": "secp256k1",
              "d": local_did_peer_keys.e_d,
              "kty": "EC",
              "x": local_did_peer_keys.e_x,
              "y": local_did_peer_keys.e_y,
          }
      }
    ]);

    let json_string = serde_json::to_string_pretty(&secrets_json)?;

    let mut file = File::create("./conf/secrets.json-generated")?;
    file.write_all(json_string.as_bytes())?;

    // Create jwt_authorization_secret
    let doc = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    println!(
        "jwt_authorization_secret = {}",
        &BASE64_URL_SAFE_NO_PAD.encode(doc.as_ref())
    );

    Ok(())
}

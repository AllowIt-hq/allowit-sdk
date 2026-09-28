#![no_std]

extern crate alloc;

use alloc::string::String;
use allowit_contract_core::{
    Error as CoreError, MAX_CHAIN_ARTIFACT_BYTES, Mandate, Request, State,
    prepare_binary_execution, record_execution, validate_binary_chain_artifact,
};
use soroban_sdk::{
    Address, Bytes, BytesN, Env, IntoVal, contract, contracterror, contractimpl, contracttype,
    token, xdr::ToXdr,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    InvalidMandate = 1,
    InvalidArtifact = 2,
    ArtifactMismatch = 3,
    Inactive = 4,
    Expired = 5,
    BindingMismatch = 6,
    Replay = 7,
    BudgetExceeded = 8,
    PolicyDenied = 9,
    UserInputRequired = 10,
    Overflow = 11,
    Unauthorized = 12,
    InvalidAccount = 13,
    AlreadyInitialized = 14,
    EvidenceRequired = 15,
    InvalidEvidence = 16,
}

impl From<CoreError> for Error {
    fn from(value: CoreError) -> Self {
        match value {
            CoreError::InvalidMandate => Self::InvalidMandate,
            CoreError::InvalidArtifact => Self::InvalidArtifact,
            CoreError::ArtifactMismatch => Self::ArtifactMismatch,
            CoreError::Inactive => Self::Inactive,
            CoreError::Expired => Self::Expired,
            CoreError::BindingMismatch => Self::BindingMismatch,
            CoreError::Replay => Self::Replay,
            CoreError::BudgetExceeded => Self::BudgetExceeded,
            CoreError::PolicyDenied => Self::PolicyDenied,
            CoreError::UserInputRequired => Self::UserInputRequired,
            CoreError::Overflow => Self::Overflow,
            CoreError::Unauthorized => Self::Unauthorized,
            CoreError::InvalidAccount => Self::InvalidAccount,
            CoreError::AlreadyInitialized => Self::AlreadyInitialized,
            CoreError::EvidenceRequired => Self::EvidenceRequired,
            CoreError::InvalidEvidence => Self::InvalidEvidence,
        }
    }
}

#[contracttype]
#[derive(Clone)]
pub struct Activation {
    pub owner: Address,
    pub executor: Address,
    pub compiler: Address,
    pub evidence_authority: Option<Address>,
    pub asset: Address,
    pub recipient: Address,
    /// Exact Borsh encoding of the shared Mandate envelope.
    pub mandate: Bytes,
    /// ALITIR01 binary compiler artifact, with the exact digest bound in mandate.
    pub artifact: Bytes,
}

#[contracttype]
#[derive(Clone)]
struct Stored {
    owner: Address,
    executor: Address,
    evidence_authority: Option<Address>,
    asset: Address,
    recipient: Address,
    state: Bytes,
}

#[contracttype]
#[derive(Clone)]
enum Key {
    Mandate(BytesN<32>),
    Latest(Address, BytesN<32>),
}

#[contract]
pub struct AllowIt;

pub fn address_identity(env: &Env, address: &Address) -> [u8; 32] {
    env.crypto().sha256(&address.to_xdr(env)).to_array()
}

pub fn address_text(address: &Address) -> Result<String, Error> {
    String::from_utf8(address.to_string().to_bytes().to_alloc_vec())
        .map_err(|_| Error::InvalidAccount)
}

fn network(env: &Env) -> Result<&'static str, Error> {
    let actual = env.ledger().network_id();
    for (label, phrase) in [
        (
            "stellar:mainnet",
            "Public Global Stellar Network ; September 2015",
        ),
        ("stellar:testnet", "Test SDF Network ; September 2015"),
    ] {
        if actual.to_array()
            == env
                .crypto()
                .sha256(&Bytes::from_slice(env, phrase.as_bytes()))
                .to_array()
        {
            return Ok(label);
        }
    }
    Err(Error::BindingMismatch)
}

/// Circle's canonical USDC issuers, verified against its asset registry.
/// Asset XDR is derived by the host into a network-specific SAC address.
pub fn usdc_asset_xdr(env: &Env) -> Result<Bytes, Error> {
    let issuer = match network(env)? {
        "stellar:mainnet" => [
            59, 153, 17, 56, 14, 254, 152, 139, 160, 168, 144, 14, 177, 207, 228, 79, 54, 111, 125,
            190, 148, 107, 237, 7, 114, 64, 247, 246, 36, 223, 21, 197,
        ],
        "stellar:testnet" => [
            66, 62, 125, 5, 242, 236, 175, 191, 236, 25, 43, 33, 90, 63, 27, 233, 106, 237, 184,
            216, 231, 2, 84, 171, 227, 65, 62, 2, 7, 222, 86, 178,
        ],
        _ => return Err(Error::BindingMismatch),
    };
    let mut xdr = [0_u8; 44];
    xdr[..4].copy_from_slice(&1_u32.to_be_bytes()); // CREDIT_ALPHANUM4
    xdr[4..8].copy_from_slice(b"USDC");
    // 8..12 is PublicKeyTypeEd25519 (zero).
    xdr[12..44].copy_from_slice(&issuer);
    Ok(Bytes::from_slice(env, &xdr))
}

fn canonical_asset(env: &Env) -> Result<Address, Error> {
    Ok(env
        .deployer()
        .with_stellar_asset(usdc_asset_xdr(env)?)
        .deployed_address())
}

fn decode(stored: &Stored) -> Result<State, Error> {
    borsh::from_slice(&stored.state.to_alloc_vec()).map_err(|_| Error::InvalidAccount)
}

fn save(env: &Env, key: &Key, stored: &mut Stored, state: &State) -> Result<(), Error> {
    let encoded = borsh::to_vec(state).map_err(|_| Error::InvalidAccount)?;
    stored.state = Bytes::from_slice(env, &encoded);
    env.storage().persistent().set(key, stored);
    // State (including revocation/nonce) must be restored if archived, never
    // silently recreated. Extend near expiry; policy expiry is independently checked.
    let ttl = env.storage().max_ttl();
    env.storage().persistent().extend_ttl(key, ttl / 2, ttl);
    env.storage().instance().extend_ttl(ttl / 2, ttl);
    Ok(())
}

#[contractimpl]
impl AllowIt {
    /// Both authorizations cover the complete invocation and therefore every
    /// envelope/artifact byte. The compiler key is explicit; there is no default.
    pub fn activate(env: Env, activation: Activation) -> Result<BytesN<32>, Error> {
        if activation.mandate.len() > 2048
            || activation.artifact.len() > MAX_CHAIN_ARTIFACT_BYTES as u32
        {
            return Err(Error::InvalidArtifact);
        }
        activation.owner.require_auth();
        activation.compiler.require_auth();
        let mandate: Mandate = borsh::from_slice(&activation.mandate.to_alloc_vec())
            .map_err(|_| Error::InvalidMandate)?;
        if mandate.owner != address_identity(&env, &activation.owner)
            || mandate.executor != address_identity(&env, &activation.executor)
            || mandate.compiler != address_identity(&env, &activation.compiler)
            || mandate.asset != address_identity(&env, &activation.asset)
            || mandate.recipient != address_identity(&env, &activation.recipient)
            || mandate.recipient_address != address_text(&activation.recipient)?
            || mandate.network != network(&env)?
            || env.ledger().timestamp() >= mandate.expires_at
        {
            return Err(Error::BindingMismatch);
        }
        if activation.asset != canonical_asset(&env)?
            || mandate.asset_decimals != 7
            || token::Client::new(&env, &activation.asset).decimals() != mandate.asset_decimals
        {
            return Err(Error::BindingMismatch);
        }
        match (&mandate.evidence_authority, &activation.evidence_authority) {
            (Some(bound), Some(address)) if bound.key == address_identity(&env, address) => {}
            (None, None) => {}
            _ => return Err(Error::BindingMismatch),
        }
        let bytes = activation.artifact.to_alloc_vec();
        validate_binary_chain_artifact(&mandate, &bytes)?;
        let id: BytesN<32> = env.crypto().sha256(&activation.mandate).into();
        let key = Key::Mandate(id.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyInitialized);
        }
        let latest_key = Key::Latest(
            activation.owner.clone(),
            BytesN::from_array(&env, &mandate.policy_id),
        );
        let latest: u64 = env.storage().persistent().get(&latest_key).unwrap_or(0);
        if latest.checked_add(1) != Some(mandate.revision) {
            return Err(Error::BindingMismatch);
        }
        env.storage()
            .persistent()
            .set(&latest_key, &mandate.revision);
        let ttl = env.storage().max_ttl();
        env.storage()
            .persistent()
            .extend_ttl(&latest_key, ttl / 2, ttl);
        let state = State {
            mandate,
            artifact: bytes,
            active: true,
            revoked: false,
            spent_units: 0,
            next_nonce: 0,
        };
        let mut stored = Stored {
            owner: activation.owner,
            executor: activation.executor,
            evidence_authority: activation.evidence_authority,
            asset: activation.asset,
            recipient: activation.recipient,
            state: Bytes::new(&env),
        };
        save(&env, &key, &mut stored, &state)?;
        Ok(id)
    }

    /// The executor signs an exact, revision-bound action. Spending uses this
    /// contract's SEP-41 allowance; no arbitrary contract invocation is exposed.
    pub fn execute(env: Env, id: BytesN<32>, request: Bytes) -> Result<u64, Error> {
        if request.len() > 2048 {
            return Err(Error::InvalidMandate);
        }
        let key = Key::Mandate(id.clone());
        let mut stored: Stored = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::InvalidAccount)?;
        stored.executor.require_auth();
        let mut state = decode(&stored)?;
        let latest_key = Key::Latest(
            stored.owner.clone(),
            BytesN::from_array(&env, &state.mandate.policy_id),
        );
        if env.storage().persistent().get::<_, u64>(&latest_key) != Some(state.mandate.revision) {
            return Err(Error::Inactive);
        }
        let ttl = env.storage().max_ttl();
        env.storage()
            .persistent()
            .extend_ttl(&latest_key, ttl / 2, ttl);
        let decoded_request: Request =
            borsh::from_slice(&request.to_alloc_vec()).map_err(|_| Error::InvalidMandate)?;
        if decoded_request.evidence.is_some() {
            let authority = stored
                .evidence_authority
                .as_ref()
                .ok_or(Error::EvidenceRequired)?;
            authority.require_auth_for_args((id, request).into_val(&env));
        }
        let request = decoded_request;
        if state.mandate.network != network(&env)?
            || stored.asset != canonical_asset(&env)?
            || state.mandate.asset != address_identity(&env, &stored.asset)
            || state.mandate.recipient != address_identity(&env, &stored.recipient)
            || token::Client::new(&env, &stored.asset).decimals() != state.mandate.asset_decimals
        {
            return Err(Error::BindingMismatch);
        }
        prepare_binary_execution(&state, &request, env.ledger().timestamp())?;
        token::Client::new(&env, &stored.asset).transfer_from(
            &env.current_contract_address(),
            &stored.owner,
            &stored.recipient,
            &i128::from(request.amount_units),
        );
        record_execution(&mut state, &request)?;
        save(&env, &key, &mut stored, &state)?;
        Ok(state.spent_units)
    }

    pub fn revoke(env: Env, id: BytesN<32>) -> Result<(), Error> {
        let key = Key::Mandate(id);
        let mut stored: Stored = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::InvalidAccount)?;
        stored.owner.require_auth();
        let mut state = decode(&stored)?;
        state.revoked = true;
        state.active = false;
        save(&env, &key, &mut stored, &state)
    }

    /// Read-only state for wallet clients: exact envelope, counters and status.
    pub fn state(env: Env, id: BytesN<32>) -> Result<Bytes, Error> {
        let stored: Stored = env
            .storage()
            .persistent()
            .get(&Key::Mandate(id))
            .ok_or(Error::InvalidAccount)?;
        Ok(stored.state)
    }
}

#[cfg(test)]
mod test;

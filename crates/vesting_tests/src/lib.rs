//! In-process test harness for the vesting escrow program.
//!
//! What runs here: the vesting program itself is compiled natively into the test binary and registered with
//! `solana-program-test` as a native builtin; the SPL Token, Token-2022 and associated token account programs are the
//! real SBF programs that `solana-program-test` bundles and runs in its BPF virtual machine. Transactions go through
//! the real bank: signature and account-lock checks, the account ownership and writability rules of the runtime, the
//! sysvars and CPI privilege checks. What does not run: the vesting program as SBF bytecode (compute units, stack and
//! heap limits are not enforced on it). `README.md` spells this out.

use solana_program::{program_pack::Pack, pubkey::Pubkey};
use solana_program_test::{processor, BanksClientError, ProgramTest, ProgramTestContext};
use solana_sdk::{
    account::{Account, AccountSharedData},
    clock::Clock,
    instruction::{AccountMeta, Instruction, InstructionError},
    signature::{Keypair, Signer},
    system_instruction,
    transaction::{Transaction, TransactionError},
};
use spl_token::state::{Account as TokenAccountState, AccountState, Mint};
use vesting_client::{self as client, InitializeParams, ScheduleAccount, VestingError};

pub use solana_program;
pub use solana_sdk;
pub use spl_token;
pub use vesting_client;

pub mod attack;

/// The fixed start of time in every test: 2023-11-14. The bank's real clock is replaced with it.
pub const T0: i64 = 1_700_000_000;
/// One hour, one day, in seconds.
pub const HOUR: i64 = 3_600;
pub const DAY: i64 = 86_400;
/// Decimals of the test mints.
pub const DECIMALS: u8 = 6;

/// The Token-2022 program id.
pub const TOKEN_2022_ID: Pubkey = vesting_escrow::token::TOKEN_2022_PROGRAM_ID;

/// Why a transaction failed, reduced to what a test asserts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The instruction at `index` returned `error`.
    Instruction { index: u8, error: InstructionError },
    /// The transaction failed before or outside an instruction (signatures, fees, account locks).
    Transaction(TransactionError),
    /// The client could not send or simulate it.
    Client(String),
}

impl Failure {
    /// The custom error code of the failing instruction, if it is one.
    pub fn custom_code(&self) -> Option<u32> {
        match self {
            Failure::Instruction { error: InstructionError::Custom(code), .. } => Some(*code),
            _ => None,
        }
    }

    /// The index of the failing instruction, if the failure is an instruction error.
    pub fn index(&self) -> Option<u8> {
        match self {
            Failure::Instruction { index, .. } => Some(*index),
            _ => None,
        }
    }

    /// One line for tables and assertion messages.
    pub fn describe(&self) -> String {
        match self {
            Failure::Instruction { index, error: InstructionError::Custom(code) } => {
                format!("instruction {index}: {}", vesting_client::describe_error(*code))
            }
            Failure::Instruction { index, error } => format!("instruction {index}: {error:?}"),
            Failure::Transaction(e) => format!("transaction: {e:?}"),
            Failure::Client(e) => format!("client: {e}"),
        }
    }
}

impl From<BanksClientError> for Failure {
    fn from(e: BanksClientError) -> Self {
        match e {
            BanksClientError::TransactionError(TransactionError::InstructionError(index, error))
            | BanksClientError::SimulationError { err: TransactionError::InstructionError(index, error), .. } => {
                Failure::Instruction { index, error }
            }
            BanksClientError::TransactionError(e) | BanksClientError::SimulationError { err: e, .. } => Failure::Transaction(e),
            other => Failure::Client(other.to_string()),
        }
    }
}

/// A running in-process bank with the vesting program and the SPL programs loaded.
pub struct Env {
    pub ctx: ProgramTestContext,
    pub payer: Keypair,
}

impl Env {
    /// Starts a bank, loads the programs and fixes the clock at [`T0`].
    pub async fn new() -> Env {
        let mut test = ProgramTest::default();
        test.add_program("vesting_escrow", vesting_escrow::ID, processor!(vesting_escrow::processor::process_instruction));
        let ctx = test.start_with_context().await;
        let payer = ctx.payer.insecure_clone();
        let mut env = Env { ctx, payer };
        env.set_time(T0).await;
        env
    }

    pub fn program_id(&self) -> Pubkey {
        vesting_escrow::ID
    }

    /// Sets the clock sysvar's Unix time. This is the only way time moves in a test.
    pub async fn set_time(&mut self, unix_timestamp: i64) {
        let mut clock: Clock = self.ctx.banks_client.get_sysvar().await.expect("clock sysvar");
        clock.unix_timestamp = unix_timestamp;
        self.ctx.set_sysvar(&clock);
    }

    /// The clock sysvar's current Unix time.
    pub async fn now(&mut self) -> i64 {
        let clock: Clock = self.ctx.banks_client.get_sysvar().await.expect("clock sysvar");
        clock.unix_timestamp
    }

    /// Sends one transaction with the payer as fee payer plus `signers`. Every call uses a fresh blockhash, so
    /// sending the same instructions twice is two transactions, not a replay.
    pub async fn send(&mut self, instructions: &[Instruction], signers: &[&Keypair]) -> Result<(), Failure> {
        let blockhash = self.ctx.get_new_latest_blockhash().await.map_err(|e| Failure::Client(e.to_string()))?;
        let mut all: Vec<&Keypair> = vec![&self.payer];
        all.extend_from_slice(signers);
        let tx = Transaction::new_signed_with_payer(instructions, Some(&self.payer.pubkey()), &all, blockhash);
        self.ctx.banks_client.process_transaction(tx).await.map_err(Failure::from)
    }

    /// Sends a transaction that is signed by exactly the given keypairs, the first of which pays the fee. Used by
    /// attacks that must control the signer set.
    pub async fn send_signed_by(&mut self, instructions: &[Instruction], fee_payer: &Keypair, signers: &[&Keypair]) -> Result<(), Failure> {
        let blockhash = self.ctx.get_new_latest_blockhash().await.map_err(|e| Failure::Client(e.to_string()))?;
        let mut all: Vec<&Keypair> = vec![fee_payer];
        all.extend_from_slice(signers);
        let tx = Transaction::new_signed_with_payer(instructions, Some(&fee_payer.pubkey()), &all, blockhash);
        self.ctx.banks_client.process_transaction(tx).await.map_err(Failure::from)
    }

    /// Sends the instructions and panics unless they succeed.
    pub async fn send_ok(&mut self, instructions: &[Instruction], signers: &[&Keypair]) {
        if let Err(failure) = self.send(instructions, signers).await {
            panic!("transaction failed: {}", failure.describe());
        }
    }

    /// Gives a wallet some lamports from the payer.
    pub async fn airdrop(&mut self, to: &Pubkey, lamports: u64) {
        let ix = system_instruction::transfer(&self.payer.pubkey(), to, lamports);
        self.send_ok(&[ix], &[]).await;
    }

    pub async fn account(&mut self, address: &Pubkey) -> Option<Account> {
        self.ctx.banks_client.get_account(*address).await.expect("rpc")
    }

    pub async fn lamports(&mut self, address: &Pubkey) -> u64 {
        self.account(address).await.map_or(0, |a| a.lamports)
    }

    /// Puts an account into the bank, replacing whatever is there (a hostile state no transaction could make).
    pub fn set_account(&mut self, address: &Pubkey, account: Account) {
        self.ctx.set_account(address, &AccountSharedData::from(account));
    }

    /// Creates a classic SPL Token mint whose authority is the payer.
    pub async fn create_mint(&mut self, decimals: u8, freeze_authority: Option<&Pubkey>) -> Pubkey {
        let mint = Keypair::new();
        let rent = self.ctx.banks_client.get_rent().await.expect("rent");
        let ixs = [
            system_instruction::create_account(
                &self.payer.pubkey(),
                &mint.pubkey(),
                rent.minimum_balance(Mint::LEN),
                Mint::LEN as u64,
                &spl_token::ID,
            ),
            spl_token::instruction::initialize_mint2(&spl_token::ID, &mint.pubkey(), &self.payer.pubkey(), freeze_authority, decimals)
                .expect("ix"),
        ];
        self.send_ok(&ixs, &[&mint]).await;
        mint.pubkey()
    }

    /// Creates a Token-2022 mint without extensions, with the real Token-2022 program.
    pub async fn create_token_2022_mint(&mut self, decimals: u8) -> Pubkey {
        let mint = Keypair::new();
        let rent = self.ctx.banks_client.get_rent().await.expect("rent");
        let create = system_instruction::create_account(
            &self.payer.pubkey(),
            &mint.pubkey(),
            rent.minimum_balance(Mint::LEN),
            Mint::LEN as u64,
            &TOKEN_2022_ID,
        );
        // InitializeMint2: tag 20, decimals, mint authority, no freeze authority
        let mut data = vec![20u8, decimals];
        data.extend_from_slice(self.payer.pubkey().as_ref());
        data.push(0);
        let init = Instruction { program_id: TOKEN_2022_ID, accounts: vec![AccountMeta::new(mint.pubkey(), false)], data };
        self.send_ok(&[create, init], &[&mint]).await;
        mint.pubkey()
    }

    /// Creates the associated token account of `owner` for `mint` (idempotent) and returns its address.
    pub async fn create_token_account(&mut self, owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        let address = client::vault_address(owner, mint);
        self.send_ok(&[client::create_vault(&self.payer.pubkey(), owner, mint)], &[]).await;
        address
    }

    /// Creates a token account at a fresh keypair address (not the associated address) with the given owner. An
    /// attacker can do this for any owner, including a schedule address.
    pub async fn create_loose_token_account(&mut self, owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        let account = Keypair::new();
        let rent = self.ctx.banks_client.get_rent().await.expect("rent");
        let ixs = [
            system_instruction::create_account(
                &self.payer.pubkey(),
                &account.pubkey(),
                rent.minimum_balance(TokenAccountState::LEN),
                TokenAccountState::LEN as u64,
                &spl_token::ID,
            ),
            spl_token::instruction::initialize_account3(&spl_token::ID, &account.pubkey(), mint, owner).expect("ix"),
        ];
        self.send_ok(&ixs, &[&account]).await;
        account.pubkey()
    }

    /// Mints `amount` to a token account; the payer is the mint authority.
    pub async fn mint_to(&mut self, mint: &Pubkey, destination: &Pubkey, amount: u64) {
        let ix = spl_token::instruction::mint_to(&spl_token::ID, mint, destination, &self.payer.pubkey(), &[], amount).expect("ix");
        self.send_ok(&[ix], &[]).await;
    }

    /// Freezes a token account (the payer must be the freeze authority of the mint).
    pub async fn freeze(&mut self, token_account: &Pubkey, mint: &Pubkey) {
        let ix = spl_token::instruction::freeze_account(&spl_token::ID, token_account, mint, &self.payer.pubkey(), &[]).expect("ix");
        self.send_ok(&[ix], &[]).await;
    }

    /// Plain SPL transfer, signed by `owner`.
    pub async fn transfer_tokens(&mut self, from: &Pubkey, to: &Pubkey, owner: &Keypair, amount: u64) {
        let ix = spl_token::instruction::transfer(&spl_token::ID, from, to, &owner.pubkey(), &[], amount).expect("ix");
        self.send_ok(&[ix], &[owner]).await;
    }

    /// The token balance of an account, zero if it does not exist.
    pub async fn balance(&mut self, token_account: &Pubkey) -> u64 {
        match self.account(token_account).await {
            Some(a) => TokenAccountState::unpack(&a.data).expect("token account").amount,
            None => 0,
        }
    }

    /// Reads a schedule account; panics if it does not exist or does not decode.
    pub async fn schedule(&mut self, address: &Pubkey) -> ScheduleAccount {
        let account = self.account(address).await.expect("schedule account exists");
        client::decode_schedule(&account.data).expect("schedule decodes")
    }

    /// Several accounts as they are now, to compare after a failed transaction.
    pub async fn snapshot(&mut self, addresses: &[Pubkey]) -> Vec<Option<Account>> {
        let mut out = Vec::with_capacity(addresses.len());
        for a in addresses {
            out.push(self.account(a).await);
        }
        out
    }

    /// Fails unless every listed account is byte-for-byte what `before` recorded.
    pub async fn assert_unchanged(&mut self, addresses: &[Pubkey], before: &[Option<Account>]) {
        let now = self.snapshot(addresses).await;
        for (i, address) in addresses.iter().enumerate() {
            assert_eq!(now[i], before[i], "account {address} changed although the transaction failed");
        }
    }
}

/// Parameters of a test schedule, relative to [`T0`].
#[derive(Clone, Debug)]
pub struct Setup {
    pub total: u64,
    /// Seconds after T0 (may be negative: a start in the past).
    pub start: i64,
    pub cliff: i64,
    pub end: i64,
    pub revocable: bool,
    pub schedule_id: u64,
    pub decimals: u8,
    /// Tokens minted to the creator; at least `total`.
    pub creator_balance: u64,
    /// True: the authority is a separate key. False: the creator is the authority.
    pub separate_authority: bool,
    pub freeze_authority: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            total: 1_000_000,
            start: 0,
            cliff: 0,
            end: 1_000,
            revocable: true,
            schedule_id: 1,
            decimals: DECIMALS,
            creator_balance: 5_000_000,
            separate_authority: true,
            freeze_authority: false,
        }
    }
}

/// A funded schedule with all its actors.
pub struct Scenario {
    pub env: Env,
    pub mint: Pubkey,
    pub creator: Keypair,
    pub beneficiary: Keypair,
    pub authority: Keypair,
    pub stranger: Keypair,
    pub creator_token: Pubkey,
    pub beneficiary_token: Pubkey,
    pub authority_token: Pubkey,
    pub stranger_token: Pubkey,
    pub schedule: Pubkey,
    pub vault: Pubkey,
    pub params: InitializeParams,
}

impl Scenario {
    /// Builds the world and runs `create_vault` + `initialize_schedule` successfully.
    pub async fn new(setup: Setup) -> Scenario {
        let mut s = Scenario::prepare(setup).await;
        s.initialize().await.expect("the schedule is created");
        s
    }

    /// Builds the world (mint, wallets, token accounts, funds) without creating the schedule.
    pub async fn prepare(setup: Setup) -> Scenario {
        let mut env = Env::new().await;
        let payer = env.payer.pubkey();
        let mint = env.create_mint(setup.decimals, setup.freeze_authority.then_some(&payer)).await;
        let creator = Keypair::new();
        let beneficiary = Keypair::new();
        let authority = if setup.separate_authority { Keypair::new() } else { creator.insecure_clone() };
        let stranger = Keypair::new();
        for k in [&creator, &beneficiary, &authority, &stranger] {
            env.airdrop(&k.pubkey(), 10_000_000_000).await;
        }
        let creator_token = env.create_token_account(&creator.pubkey(), &mint).await;
        let beneficiary_token = env.create_token_account(&beneficiary.pubkey(), &mint).await;
        let authority_token = env.create_token_account(&authority.pubkey(), &mint).await;
        let stranger_token = env.create_token_account(&stranger.pubkey(), &mint).await;
        env.mint_to(&mint, &creator_token, setup.creator_balance).await;
        let params = InitializeParams {
            creator: creator.pubkey(),
            schedule_id: setup.schedule_id,
            mint,
            source_token: creator_token,
            beneficiary: beneficiary.pubkey(),
            authority: authority.pubkey(),
            total_amount: setup.total,
            start_ts: T0 + setup.start,
            cliff_ts: T0 + setup.cliff,
            end_ts: T0 + setup.end,
            revocable: setup.revocable,
        };
        let schedule = params.schedule(&vesting_escrow::ID);
        let vault = client::vault_address(&schedule, &mint);
        Scenario {
            env,
            mint,
            creator,
            beneficiary,
            authority,
            stranger,
            creator_token,
            beneficiary_token,
            authority_token,
            stranger_token,
            schedule,
            vault,
            params,
        }
    }

    /// The instructions that create the vault and the schedule.
    pub fn initialize_instructions(&self) -> Vec<Instruction> {
        vec![
            client::create_vault(&self.creator.pubkey(), &self.schedule, &self.mint),
            client::initialize_schedule(&self.program_id(), &self.params),
        ]
    }

    pub fn program_id(&self) -> Pubkey {
        vesting_escrow::ID
    }

    /// Creates the vault and the schedule with the current params.
    pub async fn initialize(&mut self) -> Result<(), Failure> {
        let ixs = self.initialize_instructions();
        let creator = self.creator.insecure_clone();
        self.env.send(&ixs, &[&creator]).await
    }

    pub fn claim_ix(&self) -> Instruction {
        client::claim(&self.program_id(), &self.beneficiary.pubkey(), &self.schedule, &self.mint, &self.beneficiary_token)
    }

    pub fn revoke_ix(&self) -> Instruction {
        client::revoke(
            &self.program_id(),
            &self.authority.pubkey(),
            &self.schedule,
            &self.mint,
            &self.beneficiary_token,
            &self.authority_token,
        )
    }

    pub fn close_ix(&self) -> Instruction {
        client::close(&self.program_id(), &self.authority.pubkey(), &self.schedule, &self.mint, &self.authority_token)
    }

    pub fn propose_ix(&self, new_beneficiary: &Pubkey) -> Instruction {
        client::propose_beneficiary(&self.program_id(), &self.beneficiary.pubkey(), &self.schedule, new_beneficiary)
    }

    pub fn accept_ix(&self, new_beneficiary: &Pubkey) -> Instruction {
        client::accept_beneficiary(&self.program_id(), new_beneficiary, &self.schedule)
    }

    /// Moves the clock to T0 + `offset` seconds.
    pub async fn at(&mut self, offset: i64) {
        self.env.set_time(T0 + offset).await;
    }

    pub async fn claim(&mut self) -> Result<(), Failure> {
        let ix = self.claim_ix();
        let signer = self.beneficiary.insecure_clone();
        self.env.send(&[ix], &[&signer]).await
    }

    pub async fn revoke(&mut self) -> Result<(), Failure> {
        let ix = self.revoke_ix();
        let signer = self.authority.insecure_clone();
        self.env.send(&[ix], &[&signer]).await
    }

    pub async fn close(&mut self) -> Result<(), Failure> {
        let ix = self.close_ix();
        let signer = self.authority.insecure_clone();
        self.env.send(&[ix], &[&signer]).await
    }

    pub async fn state(&mut self) -> ScheduleAccount {
        let schedule = self.schedule;
        self.env.schedule(&schedule).await
    }

    pub async fn vault_balance(&mut self) -> u64 {
        let vault = self.vault;
        self.env.balance(&vault).await
    }

    pub async fn beneficiary_balance(&mut self) -> u64 {
        let token = self.beneficiary_token;
        self.env.balance(&token).await
    }

    pub async fn authority_balance(&mut self) -> u64 {
        let token = self.authority_token;
        self.env.balance(&token).await
    }

    /// Every account an attack might touch, for `snapshot` and `assert_unchanged`.
    pub fn watched(&self) -> Vec<Pubkey> {
        vec![self.schedule, self.vault, self.creator_token, self.beneficiary_token, self.authority_token, self.stranger_token]
    }
}

/// A token account state for injection with `Env::set_account`.
pub fn raw_token_account(mint: &Pubkey, owner: &Pubkey, amount: u64, state: AccountState) -> TokenAccountState {
    TokenAccountState {
        mint: *mint,
        owner: *owner,
        amount,
        delegate: solana_program::program_option::COption::None,
        state,
        is_native: solana_program::program_option::COption::None,
        delegated_amount: 0,
        close_authority: solana_program::program_option::COption::None,
    }
}

/// Wraps token account state as a rent-exempt account owned by the SPL Token program.
pub fn token_account_to_account(state: &TokenAccountState) -> Account {
    let mut data = vec![0u8; TokenAccountState::LEN];
    TokenAccountState::pack(*state, &mut data).expect("pack");
    Account { lamports: 2_039_280, data, owner: spl_token::ID, executable: false, rent_epoch: 0 }
}

/// The error a failed instruction must carry, as a `Failure` for `assert_eq!`.
pub fn expect(index: u8, error: VestingError) -> Failure {
    Failure::Instruction { index, error: InstructionError::Custom(error.code()) }
}

//! Unit tests of the byte-level contracts of the program crate: the schedule account layout, the instruction data
//! format, the error table and the address derivations. They need no runtime.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use solana_program::{program_error::ProgramError, pubkey::Pubkey};
use vesting_core::{AccountingError, ScheduleError};
use vesting_escrow::error::{lift, VestingError};
use vesting_escrow::instruction::{InitializeSchedule, VestingInstruction, INITIALIZE_DATA_LEN, PROPOSE_DATA_LEN};
use vesting_escrow::pda::{schedule_address, vault_address, ASSOCIATED_TOKEN_PROGRAM_ID, SCHEDULE_SEED};
use vesting_escrow::state::{ScheduleAccount, DISCRIMINATOR, SCHEDULE_ACCOUNT_SIZE};

fn key(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}

fn sample() -> ScheduleAccount {
    ScheduleAccount {
        bump: 254,
        revocable: true,
        revoked: false,
        id: 0x0102_0304_0506_0708,
        creator: key(1),
        authority: key(2),
        beneficiary: key(3),
        pending_beneficiary: key(4),
        mint: key(5),
        vault: key(6),
        total_amount: 1_000_000,
        claimed_amount: 250_000,
        returned_amount: 0,
        start_ts: 1_700_000_000,
        cliff_ts: 1_700_000_100,
        end_ts: 1_700_001_000,
        revoked_at: 0,
    }
}

fn packed(state: &ScheduleAccount) -> Vec<u8> {
    let mut data = vec![0u8; SCHEDULE_ACCOUNT_SIZE];
    state.pack(&mut data).unwrap();
    data
}

#[test]
fn schedule_account_layout_matches_the_documented_offsets() {
    assert_eq!(SCHEDULE_ACCOUNT_SIZE, 272);
    let data = packed(&sample());
    assert_eq!(&data[0..8], b"vestsch1");
    assert_eq!(data[8], 254, "bump");
    assert_eq!(data[9], 1, "revocable");
    assert_eq!(data[10], 0, "revoked");
    assert_eq!(&data[11..16], &[0; 5], "reserved");
    assert_eq!(&data[16..24], &0x0102_0304_0506_0708u64.to_le_bytes(), "schedule_id");
    assert_eq!(&data[24..56], &[1; 32], "creator");
    assert_eq!(&data[56..88], &[2; 32], "authority");
    assert_eq!(&data[88..120], &[3; 32], "beneficiary");
    assert_eq!(&data[120..152], &[4; 32], "pending_beneficiary");
    assert_eq!(&data[152..184], &[5; 32], "mint");
    assert_eq!(&data[184..216], &[6; 32], "vault");
    assert_eq!(&data[216..224], &1_000_000u64.to_le_bytes(), "total_amount");
    assert_eq!(&data[224..232], &250_000u64.to_le_bytes(), "claimed_amount");
    assert_eq!(&data[232..240], &0u64.to_le_bytes(), "returned_amount");
    assert_eq!(&data[240..248], &1_700_000_000i64.to_le_bytes(), "start_ts");
    assert_eq!(&data[248..256], &1_700_000_100i64.to_le_bytes(), "cliff_ts");
    assert_eq!(&data[256..264], &1_700_001_000i64.to_le_bytes(), "end_ts");
    assert_eq!(&data[264..272], &0i64.to_le_bytes(), "revoked_at");
}

#[test]
fn schedule_account_round_trips() {
    let mut state = sample();
    assert_eq!(ScheduleAccount::unpack(&packed(&state)).unwrap(), state);
    state.revoked = true;
    state.revoked_at = 1_700_000_500;
    state.returned_amount = 600_000;
    state.pending_beneficiary = Pubkey::default();
    assert_eq!(ScheduleAccount::unpack(&packed(&state)).unwrap(), state);
    assert!(!state.has_pending_beneficiary());
}

#[test]
fn schedule_account_refuses_every_wrong_length() {
    let data = packed(&sample());
    for len in [0, 1, 8, 271, 273, 544] {
        let mut wrong = data.clone();
        wrong.resize(len, 0);
        assert_eq!(ScheduleAccount::unpack(&wrong), Err(ProgramError::InvalidAccountData), "length {len}");
    }
    let mut short = vec![0u8; 271];
    assert_eq!(sample().pack(&mut short), Err(ProgramError::InvalidAccountData));
}

#[test]
fn schedule_account_refuses_a_bad_discriminator_flag_or_reserved_byte() {
    let data = packed(&sample());
    assert_eq!(
        ScheduleAccount::unpack(&[0u8; SCHEDULE_ACCOUNT_SIZE]),
        Err(ProgramError::InvalidAccountData),
        "a zeroed account is not a schedule"
    );
    for (offset, value, what) in [
        (0usize, b'x', "discriminator"),
        (7, b'2', "version"),
        (9, 2, "revocable flag"),
        (10, 7, "revoked flag"),
        (11, 1, "reserved byte"),
        (15, 1, "reserved byte"),
    ] {
        let mut wrong = data.clone();
        wrong[offset] = value;
        assert_eq!(ScheduleAccount::unpack(&wrong), Err(ProgramError::InvalidAccountData), "{what}");
    }
    assert_eq!(&DISCRIMINATOR, b"vestsch1");
}

#[test]
fn the_pure_schedule_behind_the_state_is_validated_again() {
    assert!(sample().schedule().is_ok());
    let mut zero = sample();
    zero.total_amount = 0;
    assert_eq!(zero.schedule(), Err(VestingError::CorruptSchedule));
    let mut backwards = sample();
    backwards.end_ts = backwards.start_ts;
    assert_eq!(backwards.schedule(), Err(VestingError::CorruptSchedule));
}

#[test]
fn instructions_round_trip_through_their_data() {
    let initialize = VestingInstruction::InitializeSchedule(InitializeSchedule {
        schedule_id: u64::MAX,
        beneficiary: key(7),
        authority: key(8),
        total_amount: u64::MAX,
        start_ts: 1_700_000_000,
        cliff_ts: 1_700_000_001,
        end_ts: i64::MAX,
        revocable: true,
    });
    let all = [
        initialize,
        VestingInstruction::Claim,
        VestingInstruction::Revoke,
        VestingInstruction::Close,
        VestingInstruction::ProposeBeneficiary { new_beneficiary: key(9) },
        VestingInstruction::AcceptBeneficiary,
    ];
    for instruction in all {
        let data = instruction.pack();
        assert_eq!(VestingInstruction::unpack(&data).unwrap(), instruction, "{instruction:?}");
    }
    assert_eq!(VestingInstruction::Claim.pack(), vec![1]);
    assert_eq!(VestingInstruction::ProposeBeneficiary { new_beneficiary: key(9) }.pack().len(), PROPOSE_DATA_LEN);
}

#[test]
fn initialize_data_has_the_documented_layout() {
    let data = VestingInstruction::InitializeSchedule(InitializeSchedule {
        schedule_id: 1,
        beneficiary: key(7),
        authority: key(8),
        total_amount: 2,
        start_ts: 3,
        cliff_ts: 4,
        end_ts: 5,
        revocable: false,
    })
    .pack();
    assert_eq!(data.len(), INITIALIZE_DATA_LEN);
    assert_eq!(INITIALIZE_DATA_LEN, 106);
    assert_eq!(data[0], 0);
    assert_eq!(&data[1..9], &1u64.to_le_bytes());
    assert_eq!(&data[9..41], &[7; 32]);
    assert_eq!(&data[41..73], &[8; 32]);
    assert_eq!(&data[73..81], &2u64.to_le_bytes());
    assert_eq!(&data[81..89], &3i64.to_le_bytes());
    assert_eq!(&data[89..97], &4i64.to_le_bytes());
    assert_eq!(&data[97..105], &5i64.to_le_bytes(), "end_ts");
}

#[test]
fn instruction_data_is_parsed_strictly() {
    let bad = ProgramError::InvalidInstructionData;
    assert_eq!(VestingInstruction::unpack(&[]), Err(bad.clone()), "empty");
    assert_eq!(VestingInstruction::unpack(&[6]), Err(bad.clone()), "unknown tag");
    assert_eq!(VestingInstruction::unpack(&[255]), Err(bad.clone()), "unknown tag");
    for tag in [1u8, 2, 3, 5] {
        assert_eq!(VestingInstruction::unpack(&[tag, 0]), Err(bad.clone()), "trailing byte after tag {tag}");
    }
    assert_eq!(VestingInstruction::unpack(&[4]), Err(bad.clone()), "propose without a key");
    let mut propose = vec![4u8];
    propose.extend_from_slice(&[1; 33]);
    assert_eq!(VestingInstruction::unpack(&propose), Err(bad.clone()), "propose with a long key");

    let valid = VestingInstruction::InitializeSchedule(InitializeSchedule {
        schedule_id: 1,
        beneficiary: key(7),
        authority: key(8),
        total_amount: 2,
        start_ts: 3,
        cliff_ts: 4,
        end_ts: 5,
        revocable: true,
    })
    .pack();
    for len in 0..valid.len() {
        assert_eq!(VestingInstruction::unpack(&valid[..len]), Err(bad.clone()), "initialize truncated to {len}");
    }
    let mut longer = valid.clone();
    longer.push(0);
    assert_eq!(VestingInstruction::unpack(&longer), Err(bad.clone()), "initialize with a trailing byte");
    let mut flag = valid;
    *flag.last_mut().unwrap() = 2;
    assert_eq!(VestingInstruction::unpack(&flag), Err(bad), "revocable flag of 2");
}

#[test]
fn error_codes_are_unique_dense_and_round_trip() {
    let all = VestingError::ALL;
    assert_eq!(all.len(), 37);
    for (i, error) in all.iter().enumerate() {
        assert_eq!(error.code() as usize, i, "{error:?} must have code {i}: codes are part of the interface");
        assert_eq!(VestingError::from_code(error.code()), Some(*error));
        assert!(!error.description().is_empty());
        assert_eq!(ProgramError::from(*error), ProgramError::Custom(error.code()));
    }
    assert_eq!(VestingError::from_code(37), None);
    assert_eq!(VestingError::from_code(u32::MAX), None);
    let mut texts: Vec<_> = all.iter().map(|e| e.description()).collect();
    texts.sort_unstable();
    texts.dedup();
    assert_eq!(texts.len(), all.len(), "every error has its own text");
}

#[test]
fn errors_of_the_pure_crate_map_to_distinct_program_errors() {
    let map = [
        (ScheduleError::ZeroAmount, VestingError::ZeroAmount),
        (ScheduleError::NegativeTimestamp, VestingError::NegativeTimestamp),
        (ScheduleError::EndNotAfterStart, VestingError::EndNotAfterStart),
        (ScheduleError::CliffBeforeStart, VestingError::CliffBeforeStart),
        (ScheduleError::CliffAfterEnd, VestingError::CliffAfterEnd),
        (ScheduleError::DurationTooLong, VestingError::DurationTooLong),
        (ScheduleError::AlreadyEnded, VestingError::ScheduleAlreadyEnded),
    ];
    for (from, to) in map {
        assert_eq!(VestingError::from(from), to);
        assert_eq!(lift(from), ProgramError::Custom(to.code()));
    }
    assert_eq!(VestingError::from(AccountingError::Overflow), VestingError::MathOverflow);
    assert_eq!(VestingError::from(AccountingError::ClaimedExceedsTotal), VestingError::CorruptSchedule);
    assert_eq!(VestingError::from(AccountingError::ClaimedExceedsVested), VestingError::CorruptSchedule);
}

#[test]
fn schedule_address_follows_the_documented_seeds() {
    let program = key(100);
    let (creator, mint) = (key(1), key(5));
    let (address, bump) = schedule_address(&program, &creator, &mint, 258);
    let expected =
        Pubkey::create_program_address(&[SCHEDULE_SEED, creator.as_ref(), mint.as_ref(), &[2, 1, 0, 0, 0, 0, 0, 0], &[bump]], &program)
            .unwrap();
    assert_eq!(address, expected, "seeds: \"vesting\", creator, mint, id as u64 little endian, bump");
    assert_eq!(SCHEDULE_SEED, b"vesting");
    // each seed matters
    assert_ne!(address, schedule_address(&program, &key(2), &mint, 258).0);
    assert_ne!(address, schedule_address(&program, &creator, &key(6), 258).0);
    assert_ne!(address, schedule_address(&program, &creator, &mint, 259).0);
    assert_ne!(address, schedule_address(&key(101), &creator, &mint, 258).0);
}

#[test]
fn the_schedule_bump_is_the_canonical_one() {
    let program = key(100);
    let (creator, mint) = (key(1), key(5));
    for id in 0..64u64 {
        let (address, bump) = schedule_address(&program, &creator, &mint, id);
        let id_bytes = id.to_le_bytes();
        // no higher bump gives a valid off-curve address
        for higher in (u16::from(bump) + 1)..=255 {
            let higher = [higher as u8];
            let seeds: [&[u8]; 5] = [SCHEDULE_SEED, creator.as_ref(), mint.as_ref(), &id_bytes, &higher];
            assert!(
                Pubkey::create_program_address(&seeds, &program).is_err(),
                "id {id}: a higher bump is valid, so {bump} is not canonical"
            );
        }
        assert!(!address.is_on_curve());
    }
}

#[test]
fn the_vault_is_the_associated_token_account_of_the_schedule() {
    let (schedule, mint) = (key(9), key(5));
    let expected =
        Pubkey::find_program_address(&[schedule.as_ref(), spl_token::ID.as_ref(), mint.as_ref()], &ASSOCIATED_TOKEN_PROGRAM_ID).0;
    assert_eq!(vault_address(&schedule, &mint), expected);
    assert_ne!(vault_address(&schedule, &mint), vault_address(&schedule, &key(6)));
    assert_eq!(ASSOCIATED_TOKEN_PROGRAM_ID.to_string(), "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
}

#[test]
fn the_values_the_handlers_keep_on_the_stack_stay_small() {
    // A heuristic, not a measurement: the stack frame of a function on SBF is 4 KiB and the compiler decides what is spilled.
    // These are the sizes of the values the largest handler (`initialize_schedule`) holds at once.
    use core::mem::size_of;
    let schedule = size_of::<ScheduleAccount>();
    let params = size_of::<InitializeSchedule>();
    let instruction = size_of::<VestingInstruction>();
    let token_account = size_of::<spl_token::state::Account>();
    let mint = size_of::<spl_token::state::Mint>();
    assert!(schedule <= 320, "ScheduleAccount is {schedule} bytes");
    assert!(params <= 128, "InitializeSchedule is {params} bytes");
    assert!(instruction <= 128, "VestingInstruction is {instruction} bytes");
    assert!(token_account <= 192, "a token account is {token_account} bytes");
    assert!(mint <= 96, "a mint is {mint} bytes");
    let at_once = schedule + params + instruction + 2 * token_account + mint;
    assert!(at_once <= 1_280, "{at_once} bytes held at once in initialize_schedule; the 4 KiB frame would be at risk");
    println!("sizes: schedule {schedule}, params {params}, instruction {instruction}, token account {token_account}, mint {mint}, at once {at_once}");
}

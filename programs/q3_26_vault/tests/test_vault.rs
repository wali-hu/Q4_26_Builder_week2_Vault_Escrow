use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        InstructionData, ToAccountMetas,
    },
    litesvm::LiteSVM,
    q3_26_vault::{STATE, VAULT_SEED},
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

const DEPOSIT_LAMPORTS: u64 = 500_000_000;
const WITHDRAW_LAMPORTS: u64 = 100_000_000;
const FEE: u64 = 5_000; // LiteSVM charges 5000 lamports per signature

// ---------- helpers ----------

struct Env {
    svm: LiteSVM,
    program_id: Pubkey,
}

fn setup() -> Env {
    let program_id = q3_26_vault::id();
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/q3_26_vault.so"
    ));
    svm.add_program(program_id, bytes).unwrap();
    Env { svm, program_id }
}

fn new_user(env: &mut Env) -> Keypair {
    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 2_000_000_000).unwrap();
    user
}

fn pdas(program_id: &Pubkey, owner: &Pubkey) -> (Pubkey, Pubkey) {
    let (state, _) = Pubkey::find_program_address(&[STATE, owner.as_ref()], program_id);
    let (vault, _) = Pubkey::find_program_address(&[VAULT_SEED, owner.as_ref()], program_id);
    (state, vault)
}

/// Send one instruction signed by `payer`. Ok(()) or Err(debug string of the error + logs).
fn send(env: &mut Env, payer: &Keypair, ix: Instruction) -> Result<(), String> {
    let blockhash = env.svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[payer]).unwrap();
    let result = env
        .svm
        .send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("{:?}\n{}", e.err, e.meta.logs.join("\n")));
    // New blockhash so the next identical transaction is not a duplicate.
    env.svm.expire_blockhash();
    result
}

/// Assert the transaction failed with this custom error number (Anchor or our own).
fn assert_error(result: Result<(), String>, code: u32) {
    let err = result.expect_err("transaction should have failed");
    assert!(
        err.contains(&format!("Custom({code})")),
        "expected error {code}, got:\n{err}"
    );
}

fn ix_initialize(program_id: Pubkey, user: Pubkey, state: Pubkey, vault: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id,
        &q3_26_vault::instruction::Initialize {}.data(),
        q3_26_vault::accounts::Initialize {
            user,
            vault_state: state,
            vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn ix_deposit(program_id: Pubkey, user: Pubkey, state: Pubkey, vault: Pubkey, amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        program_id,
        &q3_26_vault::instruction::Deposit { amount }.data(),
        q3_26_vault::accounts::Deposit {
            user,
            vault_state: state,
            vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn ix_withdraw(program_id: Pubkey, user: Pubkey, state: Pubkey, vault: Pubkey, amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        program_id,
        &q3_26_vault::instruction::Withdraw { amount }.data(),
        q3_26_vault::accounts::Withdraw {
            user,
            vault_state: state,
            vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn ix_close(program_id: Pubkey, user: Pubkey, state: Pubkey, vault: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id,
        &q3_26_vault::instruction::Close {}.data(),
        q3_26_vault::accounts::Close {
            user,
            vault_state: state,
            vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

/// initialize + deposit for a fresh user, returns (user, state, vault).
fn funded_vault(env: &mut Env) -> (Keypair, Pubkey, Pubkey) {
    let user = new_user(env);
    let (state, vault) = pdas(&env.program_id, &user.pubkey());
    let pid = env.program_id;
    send(env, &user, ix_initialize(pid, user.pubkey(), state, vault)).unwrap();
    send(env, &user, ix_deposit(pid, user.pubkey(), state, vault, DEPOSIT_LAMPORTS)).unwrap();
    (user, state, vault)
}

// ---------- happy path: every instruction ----------

#[test]
fn full_lifecycle_initialize_deposit_withdraw_close() {
    let mut env = setup();
    let pid = env.program_id;
    let user = new_user(&mut env);
    let (state, vault) = pdas(&pid, &user.pubkey());
    let rent_vault = env.svm.minimum_balance_for_rent_exemption(0);

    // initialize: state PDA created (2 bytes data + 8 discriminator), vault funded with rent
    send(&mut env, &user, ix_initialize(pid, user.pubkey(), state, vault)).unwrap();
    let state_acc = env.svm.get_account(&state).expect("state exists");
    assert_eq!(state_acc.data.len(), 10, "8-byte discriminator + 2 bumps");
    assert_eq!(state_acc.owner, pid);
    assert_eq!(env.svm.get_balance(&vault).unwrap(), rent_vault);
    let rent_state = state_acc.lamports;
    println!("state: {} bytes, {} lamports; vault rent: {}", state_acc.data.len(), rent_state, rent_vault);

    // deposit
    send(&mut env, &user, ix_deposit(pid, user.pubkey(), state, vault, DEPOSIT_LAMPORTS)).unwrap();
    assert_eq!(env.svm.get_balance(&vault).unwrap(), rent_vault + DEPOSIT_LAMPORTS);

    // withdraw part of it
    let before = env.svm.get_balance(&user.pubkey()).unwrap();
    send(&mut env, &user, ix_withdraw(pid, user.pubkey(), state, vault, WITHDRAW_LAMPORTS)).unwrap();
    assert_eq!(
        env.svm.get_balance(&vault).unwrap(),
        rent_vault + DEPOSIT_LAMPORTS - WITHDRAW_LAMPORTS
    );
    assert_eq!(env.svm.get_balance(&user.pubkey()).unwrap(), before + WITHDRAW_LAMPORTS - FEE);

    // close: user gets back the vault's SOL (deposit left + vault rent) + the state's rent
    let before = env.svm.get_balance(&user.pubkey()).unwrap();
    let vault_left = env.svm.get_balance(&vault).unwrap();
    send(&mut env, &user, ix_close(pid, user.pubkey(), state, vault)).unwrap();
    assert!(env.svm.get_account(&state).map_or(true, |a| a.lamports == 0), "state closed");
    assert_eq!(env.svm.get_balance(&vault).unwrap_or(0), 0, "vault emptied");
    assert_eq!(
        env.svm.get_balance(&user.pubkey()).unwrap(),
        before + vault_left + rent_state - FEE,
        "user got back vault SOL + vault rent + state rent, minus the tx fee"
    );
    assert_eq!(vault_left, DEPOSIT_LAMPORTS - WITHDRAW_LAMPORTS + rent_vault);
}

#[test]
fn withdraw_down_to_rent_minimum_works() {
    let mut env = setup();
    let pid = env.program_id;
    let (user, state, vault) = funded_vault(&mut env);
    send(&mut env, &user, ix_withdraw(pid, user.pubkey(), state, vault, DEPOSIT_LAMPORTS)).unwrap();
    assert_eq!(
        env.svm.get_balance(&vault).unwrap(),
        env.svm.minimum_balance_for_rent_exemption(0)
    );
}

#[test]
fn withdraw_full_balance_leaves_vault_at_zero_then_close_works() {
    let mut env = setup();
    let pid = env.program_id;
    let (user, state, vault) = funded_vault(&mut env);
    let all = env.svm.get_balance(&vault).unwrap(); // deposit + rent
    send(&mut env, &user, ix_withdraw(pid, user.pubkey(), state, vault, all)).unwrap();
    assert_eq!(env.svm.get_balance(&vault).unwrap_or(0), 0, "vault at 0, account gone");
    // close still works on an empty vault and returns the state rent
    send(&mut env, &user, ix_close(pid, user.pubkey(), state, vault)).unwrap();
    assert!(env.svm.get_account(&state).map_or(true, |a| a.lamports == 0));
}

#[test]
fn can_reinitialize_after_close() {
    let mut env = setup();
    let pid = env.program_id;
    let (user, state, vault) = funded_vault(&mut env);
    send(&mut env, &user, ix_close(pid, user.pubkey(), state, vault)).unwrap();
    send(&mut env, &user, ix_initialize(pid, user.pubkey(), state, vault)).unwrap();
}

// ---------- failure cases ----------

#[test]
fn deposit_zero_fails_6000_invalid_amount() {
    let mut env = setup();
    let pid = env.program_id;
    let (user, state, vault) = funded_vault(&mut env);
    assert_error(send(&mut env, &user, ix_deposit(pid, user.pubkey(), state, vault, 0)), 6000);
}

#[test]
fn withdraw_zero_fails_6000_invalid_amount() {
    let mut env = setup();
    let pid = env.program_id;
    let (user, state, vault) = funded_vault(&mut env);
    assert_error(send(&mut env, &user, ix_withdraw(pid, user.pubkey(), state, vault, 0)), 6000);
}

#[test]
fn partial_withdraw_leaving_dust_below_rent_fails_6002_would_break_rent() {
    let mut env = setup();
    let pid = env.program_id;
    let (user, state, vault) = funded_vault(&mut env);
    // one lamport more than the deposit leaves rent_min - 1: above 0, below the minimum
    assert_error(
        send(&mut env, &user, ix_withdraw(pid, user.pubkey(), state, vault, DEPOSIT_LAMPORTS + 1)),
        6002,
    );
}

#[test]
fn withdraw_more_than_balance_fails_6001_insufficient_funds() {
    let mut env = setup();
    let pid = env.program_id;
    let (user, state, vault) = funded_vault(&mut env);
    assert_error(
        send(&mut env, &user, ix_withdraw(pid, user.pubkey(), state, vault, u64::MAX)),
        6001,
    );
}

#[test]
fn withdraw_from_someone_elses_vault_fails_2006_constraint_seeds() {
    let mut env = setup();
    let pid = env.program_id;
    let (_victim, victim_state, victim_vault) = funded_vault(&mut env);
    let attacker = new_user(&mut env);
    // Attacker signs and passes the victim's accounts. The seeds are derived from
    // the signer (attacker), so they do not match the victim's addresses.
    assert_error(
        send(
            &mut env,
            &attacker,
            ix_withdraw(pid, attacker.pubkey(), victim_state, victim_vault, WITHDRAW_LAMPORTS),
        ),
        2006,
    );
}

#[test]
fn close_someone_elses_vault_fails_2006_constraint_seeds() {
    let mut env = setup();
    let pid = env.program_id;
    let (_victim, victim_state, victim_vault) = funded_vault(&mut env);
    let attacker = new_user(&mut env);
    assert_error(
        send(&mut env, &attacker, ix_close(pid, attacker.pubkey(), victim_state, victim_vault)),
        2006,
    );
}

#[test]
fn initialize_twice_fails_account_already_in_use() {
    let mut env = setup();
    let pid = env.program_id;
    let (user, state, vault) = funded_vault(&mut env);
    // System Program create_account refuses an address that already has data:
    // SystemError::AccountAlreadyInUse = Custom(0)
    assert_error(send(&mut env, &user, ix_initialize(pid, user.pubkey(), state, vault)), 0);
}

#[test]
fn withdraw_after_close_fails_3012_account_not_initialized() {
    let mut env = setup();
    let pid = env.program_id;
    let (user, state, vault) = funded_vault(&mut env);
    send(&mut env, &user, ix_close(pid, user.pubkey(), state, vault)).unwrap();
    assert_error(
        send(&mut env, &user, ix_withdraw(pid, user.pubkey(), state, vault, 1)),
        3012,
    );
}

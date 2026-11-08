use {
    anchor_lang::{
        prelude::{Clock, Pubkey},
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::associated_token::{self, get_associated_token_address},
    escrow::{state::Escrow, ESCROW_SEED},
    litesvm::LiteSVM,
    litesvm_token::{
        get_spl_account, spl_token::state::Account as TokenAccount, CreateAssociatedTokenAccount,
        CreateMint, MintTo,
    },
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

const DECIMALS: u8 = 6;
const DEPOSIT: u64 = 100_000_000; // 100 A
const RECEIVE: u64 = 50_000_000; // 50 B
const START_TS: i64 = 1_700_000_000;
const ONE_DAY: i64 = 86_400;
const SEED: u64 = 42;

// ---------- test world ----------

struct World {
    svm: LiteSVM,
    pid: Pubkey,
    maker: Keypair,
    taker: Keypair,
    mint_a: Pubkey,
    mint_b: Pubkey,
    maker_ata_a: Pubkey,
    taker_ata_b: Pubkey,
}

/// Fresh chain: escrow program, clock at START_TS, two mints, maker holds
/// 1,000 A and taker holds 1,000 B.
fn world() -> World {
    let pid = ::escrow::id();
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/escrow.so"));
    svm.add_program(pid, bytes).unwrap();
    set_time(&mut svm, START_TS);

    let maker = Keypair::new();
    let taker = Keypair::new();
    svm.airdrop(&maker.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&taker.pubkey(), 10_000_000_000).unwrap();

    let mint_a = CreateMint::new(&mut svm, &maker).decimals(DECIMALS).send().unwrap();
    let mint_b = CreateMint::new(&mut svm, &taker).decimals(DECIMALS).send().unwrap();
    let maker_ata_a = CreateAssociatedTokenAccount::new(&mut svm, &maker, &mint_a).send().unwrap();
    let taker_ata_b = CreateAssociatedTokenAccount::new(&mut svm, &taker, &mint_b).send().unwrap();
    MintTo::new(&mut svm, &maker, &mint_a, &maker_ata_a, 1_000_000_000).send().unwrap();
    MintTo::new(&mut svm, &taker, &mint_b, &taker_ata_b, 1_000_000_000).send().unwrap();

    World { svm, pid, maker, taker, mint_a, mint_b, maker_ata_a, taker_ata_b }
}

fn set_time(svm: &mut LiteSVM, ts: i64) {
    let mut clock: Clock = svm.get_sysvar();
    clock.unix_timestamp = ts;
    svm.set_sysvar(&clock);
}

fn escrow_pda(pid: &Pubkey, maker: &Pubkey, seed: u64) -> Pubkey {
    Pubkey::find_program_address(&[ESCROW_SEED, maker.as_ref(), &seed.to_le_bytes()], pid).0
}

fn balance(svm: &LiteSVM, ata: &Pubkey) -> u64 {
    get_spl_account::<TokenAccount>(svm, ata).map(|a| a.amount).unwrap_or(0)
}

fn exists(svm: &LiteSVM, addr: &Pubkey) -> bool {
    svm.get_account(addr).map_or(false, |a| a.lamports > 0)
}

fn read_escrow(svm: &LiteSVM, addr: &Pubkey) -> Escrow {
    let acc = svm.get_account(addr).expect("escrow account");
    Escrow::try_deserialize(&mut acc.data.as_slice()).unwrap()
}

fn send(svm: &mut LiteSVM, signer: &Keypair, ix: Instruction) -> Result<(), String> {
    let msg = Message::new_with_blockhash(&[ix], Some(&signer.pubkey()), &svm.latest_blockhash());
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[signer]).unwrap();
    let res = svm
        .send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("{:?}\n{}", e.err, e.meta.logs.join("\n")));
    svm.expire_blockhash();
    res
}

fn assert_error(res: Result<(), String>, code: u32) {
    let err = res.expect_err("transaction should have failed");
    assert!(err.contains(&format!("Custom({code})")), "expected {code}, got:\n{err}");
}

// ---------- instruction builders ----------

fn ix_make(w: &World, maker: Pubkey, seed: u64, deposit: u64, receive: u64, deadline: i64, taker: Option<Pubkey>) -> Instruction {
    let escrow = escrow_pda(&w.pid, &maker, seed);
    Instruction::new_with_bytes(
        w.pid,
        &::escrow::instruction::Make { seed, deposit, receive, deadline, taker }.data(),
        ::escrow::accounts::Make {
            maker,
            mint_a: w.mint_a,
            mint_b: w.mint_b,
            maker_ata_a: get_associated_token_address(&maker, &w.mint_a),
            escrow,
            vault: get_associated_token_address(&escrow, &w.mint_a),
            associated_token_program: associated_token::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn ix_take(w: &World, taker: Pubkey, mint_b: Pubkey, expected_receive: u64) -> Instruction {
    let maker = w.maker.pubkey();
    let escrow = escrow_pda(&w.pid, &maker, SEED);
    Instruction::new_with_bytes(
        w.pid,
        &::escrow::instruction::Take { expected_receive }.data(),
        ::escrow::accounts::Take {
            taker,
            maker,
            mint_a: w.mint_a,
            mint_b,
            taker_ata_a: get_associated_token_address(&taker, &w.mint_a),
            taker_ata_b: get_associated_token_address(&taker, &mint_b),
            maker_ata_b: get_associated_token_address(&maker, &mint_b),
            escrow,
            vault: get_associated_token_address(&escrow, &w.mint_a),
            associated_token_program: associated_token::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

/// `caller` signs; `maker` is the account passed as the maker. The escrow PDA is
/// always derived from `maker`, like a client would do.
fn ix_refund(w: &World, caller: Pubkey, maker: Pubkey) -> Instruction {
    let escrow = escrow_pda(&w.pid, &maker, SEED);
    Instruction::new_with_bytes(
        w.pid,
        &::escrow::instruction::Refund {}.data(),
        ::escrow::accounts::Refund {
            caller,
            maker,
            mint_a: w.mint_a,
            maker_ata_a: get_associated_token_address(&maker, &w.mint_a),
            escrow,
            vault: get_associated_token_address(&escrow, &w.mint_a),
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn ix_update(w: &World, signer: Pubkey, new_receive: u64, new_deadline: i64) -> Instruction {
    let escrow = escrow_pda(&w.pid, &w.maker.pubkey(), SEED);
    Instruction::new_with_bytes(
        w.pid,
        &::escrow::instruction::Update { new_receive, new_deadline }.data(),
        ::escrow::accounts::Update { maker: signer, escrow }.to_account_metas(None),
    )
}

/// Open the standard offer: 100 A for 50 B, one day, anyone can take.
fn make_offer(w: &mut World, taker: Option<Pubkey>) {
    let ix = ix_make(w, w.maker.pubkey(), SEED, DEPOSIT, RECEIVE, START_TS + ONE_DAY, taker);
    let maker = w.maker.insecure_clone();
    send(&mut w.svm, &maker, ix).unwrap();
}

// ---------- make ----------

#[test]
fn make_locks_tokens_and_stores_terms() {
    let mut w = world();
    make_offer(&mut w, None);

    let escrow = escrow_pda(&w.pid, &w.maker.pubkey(), SEED);
    let vault = get_associated_token_address(&escrow, &w.mint_a);
    let state = read_escrow(&w.svm, &escrow);
    assert_eq!(state.maker, w.maker.pubkey());
    assert_eq!(state.mint_a, w.mint_a);
    assert_eq!(state.mint_b, w.mint_b);
    assert_eq!(state.receive, RECEIVE);
    assert_eq!(state.deadline, START_TS + ONE_DAY);
    assert_eq!(state.taker, None);
    assert_eq!(balance(&w.svm, &vault), DEPOSIT);
    assert_eq!(balance(&w.svm, &w.maker_ata_a), 1_000_000_000 - DEPOSIT);

    let acc = w.svm.get_account(&escrow).unwrap();
    println!("escrow account: {} bytes, {} lamports", acc.data.len(), acc.lamports);
}

#[test]
fn make_with_past_deadline_fails_6001_invalid_deadline() {
    let mut w = world();
    let ix = ix_make(&w, w.maker.pubkey(), SEED, DEPOSIT, RECEIVE, START_TS - 1, None);
    let maker = w.maker.insecure_clone();
    assert_error(send(&mut w.svm, &maker, ix), 6001);
}

#[test]
fn make_with_zero_amounts_fails_6000_invalid_amount() {
    let mut w = world();
    let maker = w.maker.insecure_clone();
    let ix = ix_make(&w, maker.pubkey(), SEED, DEPOSIT, 0, START_TS + ONE_DAY, None);
    assert_error(send(&mut w.svm, &maker, ix), 6000);
    let ix = ix_make(&w, maker.pubkey(), SEED, 0, RECEIVE, START_TS + ONE_DAY, None);
    assert_error(send(&mut w.svm, &maker, ix), 6000);
}

// ---------- take ----------

#[test]
fn take_swaps_tokens_and_closes_escrow() {
    let mut w = world();
    make_offer(&mut w, None);
    let escrow = escrow_pda(&w.pid, &w.maker.pubkey(), SEED);
    let vault = get_associated_token_address(&escrow, &w.mint_a);
    let maker_sol_before = w.svm.get_balance(&w.maker.pubkey()).unwrap();
    let rent_back = w.svm.get_balance(&escrow).unwrap() + w.svm.get_balance(&vault).unwrap();

    let taker = w.taker.insecure_clone();
    let ix = ix_take(&w, taker.pubkey(), w.mint_b, RECEIVE);
    send(&mut w.svm, &taker, ix).unwrap();

    // Taker got all of A, maker got exactly `receive` of B.
    let taker_ata_a = get_associated_token_address(&taker.pubkey(), &w.mint_a);
    let maker_ata_b = get_associated_token_address(&w.maker.pubkey(), &w.mint_b);
    assert_eq!(balance(&w.svm, &taker_ata_a), DEPOSIT);
    assert_eq!(balance(&w.svm, &maker_ata_b), RECEIVE);
    assert_eq!(balance(&w.svm, &w.taker_ata_b), 1_000_000_000 - RECEIVE);
    // Escrow and vault are gone; their rent went back to the maker (the taker paid fees).
    assert!(!exists(&w.svm, &escrow));
    assert!(!exists(&w.svm, &vault));
    assert_eq!(w.svm.get_balance(&w.maker.pubkey()).unwrap(), maker_sol_before + rent_back);
}

#[test]
fn take_after_deadline_fails_6002_escrow_expired() {
    let mut w = world();
    make_offer(&mut w, None);
    set_time(&mut w.svm, START_TS + ONE_DAY); // deadline is exclusive
    let taker = w.taker.insecure_clone();
    let ix = ix_take(&w, taker.pubkey(), w.mint_b, RECEIVE);
    assert_error(send(&mut w.svm, &taker, ix), 6002);
}

#[test]
fn take_with_stale_price_fails_6004_terms_changed() {
    let mut w = world();
    make_offer(&mut w, None);
    let maker = w.maker.insecure_clone();
    let ix = ix_update(&w, maker.pubkey(), RECEIVE * 2, START_TS + ONE_DAY);
    send(&mut w.svm, &maker, ix).unwrap();
    // Taker still believes the price is RECEIVE: refused instead of overpaying.
    let taker = w.taker.insecure_clone();
    let ix = ix_take(&w, taker.pubkey(), w.mint_b, RECEIVE);
    assert_error(send(&mut w.svm, &taker, ix), 6004);
}

#[test]
fn take_with_wrong_mint_b_fails_2001_has_one() {
    let mut w = world();
    make_offer(&mut w, None);
    // Taker tries to pay with a worthless token they minted themselves.
    let taker = w.taker.insecure_clone();
    let fake = CreateMint::new(&mut w.svm, &taker).decimals(DECIMALS).send().unwrap();
    let fake_ata = CreateAssociatedTokenAccount::new(&mut w.svm, &taker, &fake).send().unwrap();
    MintTo::new(&mut w.svm, &taker, &fake, &fake_ata, 1_000_000_000).send().unwrap();
    let ix = ix_take(&w, taker.pubkey(), fake, RECEIVE);
    assert_error(send(&mut w.svm, &taker, ix), 2001);
}

#[test]
/// Anchor 1.x stops this before our handler: taker == maker makes taker_ata_b and
/// maker_ata_b the same mutable account -> 2040 ConstraintDuplicateMutableAccount.
/// Our own MakerCannotTake (6005) check stays as defense in depth.
fn maker_cannot_take_own_offer_fails_2040_duplicate_mutable_account() {
    let mut w = world();
    make_offer(&mut w, None);
    let maker = w.maker.insecure_clone();
    // Give the maker some B so the accounts load and the handler's check runs.
    let maker_ata_b = CreateAssociatedTokenAccount::new(&mut w.svm, &maker, &w.mint_b).send().unwrap();
    let taker = w.taker.insecure_clone();
    MintTo::new(&mut w.svm, &taker, &w.mint_b, &maker_ata_b, RECEIVE).send().unwrap();
    let ix = ix_take(&w, maker.pubkey(), w.mint_b, RECEIVE);
    assert_error(send(&mut w.svm, &maker, ix), 2040);
}

// ---------- timed escrow extension: designated taker ----------

#[test]
fn only_designated_taker_can_take() {
    let mut w = world();
    let designated = w.taker.pubkey();
    make_offer(&mut w, Some(designated));

    // A stranger with B tokens is refused.
    let stranger = Keypair::new();
    w.svm.airdrop(&stranger.pubkey(), 10_000_000_000).unwrap();
    let ata = CreateAssociatedTokenAccount::new(&mut w.svm, &stranger, &w.mint_b).send().unwrap();
    let taker = w.taker.insecure_clone();
    MintTo::new(&mut w.svm, &taker, &w.mint_b, &ata, RECEIVE).send().unwrap();
    let ix = ix_take(&w, stranger.pubkey(), w.mint_b, RECEIVE);
    assert_error(send(&mut w.svm, &stranger, ix), 6003);

    // The named taker succeeds.
    let ix = ix_take(&w, taker.pubkey(), w.mint_b, RECEIVE);
    send(&mut w.svm, &taker, ix).unwrap();
}

// ---------- refund ----------

#[test]
fn refund_returns_tokens_and_closes_escrow() {
    let mut w = world();
    make_offer(&mut w, None);
    let escrow = escrow_pda(&w.pid, &w.maker.pubkey(), SEED);
    let vault = get_associated_token_address(&escrow, &w.mint_a);

    let maker = w.maker.insecure_clone();
    let ix = ix_refund(&w, maker.pubkey(), maker.pubkey());
    send(&mut w.svm, &maker, ix).unwrap();

    assert_eq!(balance(&w.svm, &w.maker_ata_a), 1_000_000_000);
    assert!(!exists(&w.svm, &escrow));
    assert!(!exists(&w.svm, &vault));
}

#[test]
fn refund_still_works_after_deadline() {
    let mut w = world();
    make_offer(&mut w, None);
    set_time(&mut w.svm, START_TS + 2 * ONE_DAY);
    let maker = w.maker.insecure_clone();
    let ix = ix_refund(&w, maker.pubkey(), maker.pubkey());
    send(&mut w.svm, &maker, ix).unwrap();
    assert_eq!(balance(&w.svm, &w.maker_ata_a), 1_000_000_000);
}

#[test]
fn refund_by_someone_else_before_deadline_fails_6006() {
    let mut w = world();
    make_offer(&mut w, None);
    let taker = w.taker.insecure_clone();
    let ix = ix_refund(&w, taker.pubkey(), w.maker.pubkey());
    assert_error(send(&mut w.svm, &taker, ix), 6006);
}

#[test]
fn anyone_can_refund_after_deadline_and_tokens_go_to_maker() {
    let mut w = world();
    make_offer(&mut w, None);
    let escrow = escrow_pda(&w.pid, &w.maker.pubkey(), SEED);
    let maker_sol_before = w.svm.get_balance(&w.maker.pubkey()).unwrap();
    let vault = get_associated_token_address(&escrow, &w.mint_a);
    let rent_back = w.svm.get_balance(&escrow).unwrap() + w.svm.get_balance(&vault).unwrap();

    set_time(&mut w.svm, START_TS + ONE_DAY);
    let keeper = Keypair::new();
    w.svm.airdrop(&keeper.pubkey(), 1_000_000_000).unwrap();
    let ix = ix_refund(&w, keeper.pubkey(), w.maker.pubkey());
    send(&mut w.svm, &keeper, ix).unwrap();

    assert_eq!(balance(&w.svm, &w.maker_ata_a), 1_000_000_000, "all A back to the maker");
    assert!(!exists(&w.svm, &escrow));
    assert_eq!(
        w.svm.get_balance(&w.maker.pubkey()).unwrap(),
        maker_sol_before + rent_back,
        "rent to the maker, the keeper only paid the fee"
    );
}

#[test]
fn refund_claiming_to_be_the_maker_fails_2006_constraint_seeds() {
    let mut w = world();
    make_offer(&mut w, None);
    // After the deadline, an attacker passes themselves as "maker" with the real
    // escrow, hoping to receive the tokens. Seeds from the fake maker do not match.
    set_time(&mut w.svm, START_TS + ONE_DAY);
    let taker = w.taker.insecure_clone();
    CreateAssociatedTokenAccount::new(&mut w.svm, &taker, &w.mint_a).send().unwrap();
    let real_escrow = escrow_pda(&w.pid, &w.maker.pubkey(), SEED);
    let mut ix = ix_refund(&w, taker.pubkey(), taker.pubkey());
    // point the escrow and vault metas at the real offer
    ix.accounts[4].pubkey = real_escrow;
    ix.accounts[5].pubkey = get_associated_token_address(&real_escrow, &w.mint_a);
    assert_error(send(&mut w.svm, &taker, ix), 2006);
}

// ---------- update ----------

#[test]
fn update_changes_price_and_deadline_then_take_uses_new_price() {
    let mut w = world();
    make_offer(&mut w, None);
    let escrow = escrow_pda(&w.pid, &w.maker.pubkey(), SEED);

    let maker = w.maker.insecure_clone();
    let new_receive = 80_000_000;
    let new_deadline = START_TS + 7 * ONE_DAY;
    let ix = ix_update(&w, maker.pubkey(), new_receive, new_deadline);
    send(&mut w.svm, &maker, ix).unwrap();

    let state = read_escrow(&w.svm, &escrow);
    assert_eq!(state.receive, new_receive);
    assert_eq!(state.deadline, new_deadline);

    // After the old deadline but before the new one, take works at the new price.
    set_time(&mut w.svm, START_TS + 3 * ONE_DAY);
    let taker = w.taker.insecure_clone();
    let ix = ix_take(&w, taker.pubkey(), w.mint_b, new_receive);
    send(&mut w.svm, &taker, ix).unwrap();
    let maker_ata_b = get_associated_token_address(&maker.pubkey(), &w.mint_b);
    assert_eq!(balance(&w.svm, &maker_ata_b), new_receive);
}

#[test]
fn update_by_someone_else_fails_2006_constraint_seeds() {
    let mut w = world();
    make_offer(&mut w, None);
    let taker = w.taker.insecure_clone();
    let ix = ix_update(&w, taker.pubkey(), 1, START_TS + ONE_DAY);
    assert_error(send(&mut w.svm, &taker, ix), 2006);
}

#[test]
fn update_after_deadline_fails_6002_escrow_expired() {
    let mut w = world();
    make_offer(&mut w, None);
    set_time(&mut w.svm, START_TS + 2 * ONE_DAY);
    let maker = w.maker.insecure_clone();
    let ix = ix_update(&w, maker.pubkey(), RECEIVE, START_TS + 5 * ONE_DAY);
    assert_error(send(&mut w.svm, &maker, ix), 6002);
}

#[test]
fn update_with_bad_values_fails_6000_and_6001() {
    let mut w = world();
    make_offer(&mut w, None);
    let maker = w.maker.insecure_clone();
    let ix = ix_update(&w, maker.pubkey(), 0, START_TS + ONE_DAY);
    assert_error(send(&mut w.svm, &maker, ix), 6000);
    let ix = ix_update(&w, maker.pubkey(), RECEIVE, START_TS - 1);
    assert_error(send(&mut w.svm, &maker, ix), 6001);
}

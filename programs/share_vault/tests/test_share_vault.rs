use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        InstructionData, ToAccountMetas,
    },
    anchor_spl::associated_token::{self, get_associated_token_address},
    litesvm::LiteSVM,
    litesvm_token::{
        get_spl_account,
        spl_token::state::{Account as TokenAccount, Mint},
        CreateAssociatedTokenAccount, CreateMint, MintTo,
    },
    share_vault::{SHARES_SEED, VAULT_SEED},
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

const DECIMALS: u8 = 6;
const UNIT: u64 = 1_000_000; // 1 token

// ---------- test world ----------

struct World {
    svm: LiteSVM,
    pid: Pubkey,
    manager: Keypair,
    market: Keypair,
    alice: Keypair,
    bob: Keypair,
    asset_mint: Pubkey,
    position_mint: Pubkey,
    vault: Pubkey,
    share_mint: Pubkey,
}

impl World {
    fn asset_reserve(&self) -> Pubkey {
        get_associated_token_address(&self.vault, &self.asset_mint)
    }
    fn position_reserve(&self) -> Pubkey {
        get_associated_token_address(&self.vault, &self.position_mint)
    }
}

/// Vault initialized. Alice and Bob each hold 1,000 asset. The market holds
/// 10,000 position tokens (it issues them when the vault deploys liquidity).
fn world() -> World {
    let pid = share_vault::id();
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/share_vault.so"));
    svm.add_program(pid, bytes).unwrap();

    let (manager, market, alice, bob) = (Keypair::new(), Keypair::new(), Keypair::new(), Keypair::new());
    for k in [&manager, &market, &alice, &bob] {
        svm.airdrop(&k.pubkey(), 10_000_000_000).unwrap();
    }

    let asset_mint = CreateMint::new(&mut svm, &manager).decimals(DECIMALS).send().unwrap();
    let position_mint = CreateMint::new(&mut svm, &market).decimals(DECIMALS).send().unwrap();
    for user in [&alice, &bob] {
        let ata = CreateAssociatedTokenAccount::new(&mut svm, user, &asset_mint).send().unwrap();
        MintTo::new(&mut svm, &manager, &asset_mint, &ata, 1_000 * UNIT).send().unwrap();
    }
    CreateAssociatedTokenAccount::new(&mut svm, &market, &asset_mint).send().unwrap();
    let mp = CreateAssociatedTokenAccount::new(&mut svm, &market, &position_mint).send().unwrap();
    MintTo::new(&mut svm, &market, &position_mint, &mp, 10_000 * UNIT).send().unwrap();

    let vault = Pubkey::find_program_address(&[VAULT_SEED, manager.pubkey().as_ref()], &pid).0;
    let share_mint = Pubkey::find_program_address(&[SHARES_SEED, vault.as_ref()], &pid).0;

    let mut w = World { svm, pid, manager, market, alice, bob, asset_mint, position_mint, vault, share_mint };
    let ix = Instruction::new_with_bytes(
        w.pid,
        &share_vault::instruction::Initialize {}.data(),
        share_vault::accounts::Initialize {
            manager: w.manager.pubkey(),
            asset_mint,
            position_mint,
            vault,
            share_mint,
            asset_reserve: w.asset_reserve(),
            position_reserve: w.position_reserve(),
            associated_token_program: associated_token::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );
    let m = w.manager.insecure_clone();
    send(&mut w.svm, &[&m], ix).unwrap();
    w
}

fn send(svm: &mut LiteSVM, signers: &[&Keypair], ix: Instruction) -> Result<(), String> {
    let msg = Message::new_with_blockhash(&[ix], Some(&signers[0].pubkey()), &svm.latest_blockhash());
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
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

fn bal(svm: &LiteSVM, ata: &Pubkey) -> u64 {
    get_spl_account::<TokenAccount>(svm, ata).map(|a| a.amount).unwrap_or(0)
}

fn supply(w: &World) -> u64 {
    get_spl_account::<Mint>(&w.svm, &w.share_mint).unwrap().supply
}

fn shares_of(w: &World, user: &Pubkey) -> u64 {
    bal(&w.svm, &get_associated_token_address(user, &w.share_mint))
}

// ---------- instruction builders ----------

fn ix_deposit(w: &World, user: Pubkey, amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        w.pid,
        &share_vault::instruction::Deposit { amount }.data(),
        share_vault::accounts::Deposit {
            user,
            vault: w.vault,
            asset_mint: w.asset_mint,
            position_mint: w.position_mint,
            share_mint: w.share_mint,
            asset_reserve: w.asset_reserve(),
            position_reserve: w.position_reserve(),
            user_asset: get_associated_token_address(&user, &w.asset_mint),
            user_shares: get_associated_token_address(&user, &w.share_mint),
            associated_token_program: associated_token::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn ix_deploy(w: &World, manager: Pubkey, amount: u64) -> Instruction {
    let vault = Pubkey::find_program_address(&[VAULT_SEED, manager.as_ref()], &w.pid).0;
    let market = w.market.pubkey();
    let mut ix = Instruction::new_with_bytes(
        w.pid,
        &share_vault::instruction::Deploy { amount }.data(),
        share_vault::accounts::Deploy {
            manager,
            market,
            vault,
            asset_mint: w.asset_mint,
            position_mint: w.position_mint,
            asset_reserve: w.asset_reserve(),
            position_reserve: w.position_reserve(),
            market_asset: get_associated_token_address(&market, &w.asset_mint),
            market_position: get_associated_token_address(&market, &w.position_mint),
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
    );
    // always target the real vault, whoever claims to be the manager
    ix.accounts[2].pubkey = w.vault;
    ix
}

/// `user` signs; `owner` is whose share account is passed (normally the same).
fn ix_withdraw(w: &World, user: Pubkey, owner: Pubkey, shares: u64) -> Instruction {
    Instruction::new_with_bytes(
        w.pid,
        &share_vault::instruction::Withdraw { shares }.data(),
        share_vault::accounts::Withdraw {
            user,
            vault: w.vault,
            asset_mint: w.asset_mint,
            position_mint: w.position_mint,
            share_mint: w.share_mint,
            asset_reserve: w.asset_reserve(),
            position_reserve: w.position_reserve(),
            user_asset: get_associated_token_address(&user, &w.asset_mint),
            user_shares: get_associated_token_address(&owner, &w.share_mint),
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
    )
}

fn ix_redeem(w: &World, user: Pubkey, shares: u64) -> Instruction {
    Instruction::new_with_bytes(
        w.pid,
        &share_vault::instruction::RedeemInKind { shares }.data(),
        share_vault::accounts::RedeemInKind {
            user,
            vault: w.vault,
            asset_mint: w.asset_mint,
            position_mint: w.position_mint,
            share_mint: w.share_mint,
            asset_reserve: w.asset_reserve(),
            position_reserve: w.position_reserve(),
            user_asset: get_associated_token_address(&user, &w.asset_mint),
            user_position: get_associated_token_address(&user, &w.position_mint),
            user_shares: get_associated_token_address(&user, &w.share_mint),
            associated_token_program: associated_token::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn deposit(w: &mut World, who: &Keypair, amount: u64) {
    let ix = ix_deposit(w, who.pubkey(), amount);
    send(&mut w.svm, &[who], ix).unwrap();
}

fn deploy(w: &mut World, amount: u64) {
    let ix = ix_deploy(w, w.manager.pubkey(), amount);
    let (m, k) = (w.manager.insecure_clone(), w.market.insecure_clone());
    send(&mut w.svm, &[&m, &k], ix).unwrap();
}

/// Alice deposits 100, Bob deposits 50.
fn funded() -> (World, Keypair, Keypair) {
    let mut w = world();
    let (a, b) = (w.alice.insecure_clone(), w.bob.insecure_clone());
    deposit(&mut w, &a, 100 * UNIT);
    deposit(&mut w, &b, 50 * UNIT);
    (w, a, b)
}

// ---------- initialize + deposit ----------

#[test]
fn initialize_creates_vault_reserves_and_share_mint() {
    let w = world();
    let mint = get_spl_account::<Mint>(&w.svm, &w.share_mint).unwrap();
    assert_eq!(mint.decimals, DECIMALS);
    assert_eq!(mint.mint_authority, Some(w.vault).into(), "only the vault PDA can mint shares");
    assert_eq!(mint.supply, 0);
    assert!(w.svm.get_account(&w.asset_reserve()).is_some());
    assert!(w.svm.get_account(&w.position_reserve()).is_some());
}

#[test]
fn deposit_mints_shares_one_to_one_then_pro_rata() {
    let (mut w, alice, bob) = funded();
    assert_eq!(shares_of(&w, &alice.pubkey()), 100 * UNIT);
    assert_eq!(shares_of(&w, &bob.pubkey()), 50 * UNIT);
    assert_eq!(bal(&w.svm, &w.asset_reserve()), 150 * UNIT);

    // After deploy the value is unchanged (1:1), so a new deposit is still fairly priced.
    deploy(&mut w, 120 * UNIT);
    deposit(&mut w, &alice, 30 * UNIT);
    assert_eq!(shares_of(&w, &alice.pubkey()), 130 * UNIT);
    assert_eq!(supply(&w), 180 * UNIT);
}

#[test]
fn deposit_zero_fails_6000() {
    let mut w = world();
    let alice = w.alice.insecure_clone();
    let ix = ix_deposit(&w, alice.pubkey(), 0);
    assert_error(send(&mut w.svm, &[&alice], ix), 6000);
}

// ---------- deploy (manager) ----------

#[test]
fn deploy_swaps_idle_asset_for_position_one_to_one() {
    let (mut w, _, _) = funded();
    deploy(&mut w, 150 * UNIT);
    assert_eq!(bal(&w.svm, &w.asset_reserve()), 0, "no idle liquidity left");
    assert_eq!(bal(&w.svm, &w.position_reserve()), 150 * UNIT);
    // The manager received nothing: the asset went to the market.
    let market_asset = get_associated_token_address(&w.market.pubkey(), &w.asset_mint);
    assert_eq!(bal(&w.svm, &market_asset), 150 * UNIT);
    let manager_asset = get_associated_token_address(&w.manager.pubkey(), &w.asset_mint);
    assert_eq!(bal(&w.svm, &manager_asset), 0);
}

#[test]
fn deploy_by_non_manager_fails_2006_constraint_seeds() {
    let (mut w, alice, _) = funded();
    // Alice claims to be the manager of the real vault: seeds [vault, alice] do not match.
    let ix = ix_deploy(&w, alice.pubkey(), 10 * UNIT);
    let market = w.market.insecure_clone();
    assert_error(send(&mut w.svm, &[&alice, &market], ix), 2006);
}

#[test]
fn deploy_more_than_idle_fails_6001() {
    let (mut w, _, _) = funded();
    let ix = ix_deploy(&w, w.manager.pubkey(), 151 * UNIT);
    let (m, k) = (w.manager.insecure_clone(), w.market.insecure_clone());
    assert_error(send(&mut w.svm, &[&m, &k], ix), 6001);
}

// ---------- cash withdraw ----------

#[test]
fn withdraw_pays_cash_while_liquid() {
    let (mut w, alice, _) = funded();
    let ix = ix_withdraw(&w, alice.pubkey(), alice.pubkey(), 40 * UNIT);
    send(&mut w.svm, &[&alice], ix).unwrap();
    let alice_asset = get_associated_token_address(&alice.pubkey(), &w.asset_mint);
    assert_eq!(bal(&w.svm, &alice_asset), 940 * UNIT);
    assert_eq!(shares_of(&w, &alice.pubkey()), 60 * UNIT);
    assert_eq!(supply(&w), 110 * UNIT);
}

#[test]
fn withdraw_without_liquidity_fails_6001_not_enough_liquidity() {
    let (mut w, alice, _) = funded();
    deploy(&mut w, 150 * UNIT);
    let ix = ix_withdraw(&w, alice.pubkey(), alice.pubkey(), 10 * UNIT);
    assert_error(send(&mut w.svm, &[&alice], ix), 6001);
}

#[test]
fn withdraw_someone_elses_shares_fails_2015_token_owner() {
    let (mut w, alice, bob) = funded();
    // Bob passes Alice's share account. associated_token::authority = user first
    // checks the token account owner: 2015 ConstraintTokenOwner.
    let ix = ix_withdraw(&w, bob.pubkey(), alice.pubkey(), 10 * UNIT);
    assert_error(send(&mut w.svm, &[&bob], ix), 2015);
}

// ---------- redeem in kind: the always-open exit ----------

#[test]
fn redeem_in_kind_works_with_zero_liquidity() {
    let (mut w, alice, _) = funded();
    deploy(&mut w, 150 * UNIT); // everything is in the market position
    let ix = ix_redeem(&w, alice.pubkey(), 100 * UNIT);
    send(&mut w.svm, &[&alice], ix).unwrap();
    // Alice owned 100/150 of the vault, so she gets 100/150 of the 150 position tokens.
    let alice_position = get_associated_token_address(&alice.pubkey(), &w.position_mint);
    assert_eq!(bal(&w.svm, &alice_position), 100 * UNIT);
    assert_eq!(shares_of(&w, &alice.pubkey()), 0);
    assert_eq!(bal(&w.svm, &w.position_reserve()), 50 * UNIT);
}

#[test]
fn redeem_in_kind_pays_a_pro_rata_slice_of_every_reserve() {
    let (mut w, _, bob) = funded();
    deploy(&mut w, 90 * UNIT); // idle 60, position 90
    let ix = ix_redeem(&w, bob.pubkey(), 50 * UNIT); // Bob owns 50/150 = 1/3
    send(&mut w.svm, &[&bob], ix).unwrap();
    let bob_asset = get_associated_token_address(&bob.pubkey(), &w.asset_mint);
    let bob_position = get_associated_token_address(&bob.pubkey(), &w.position_mint);
    assert_eq!(bal(&w.svm, &bob_asset), 950 * UNIT + 20 * UNIT, "1/3 of 60 idle");
    assert_eq!(bal(&w.svm, &bob_position), 30 * UNIT, "1/3 of 90 position");
}

#[test]
fn everyone_can_exit_and_the_vault_ends_empty() {
    let (mut w, alice, bob) = funded();
    deploy(&mut w, 150 * UNIT);
    for (who, shares) in [(&alice, 100 * UNIT), (&bob, 50 * UNIT)] {
        let ix = ix_redeem(&w, who.pubkey(), shares);
        send(&mut w.svm, &[who], ix).unwrap();
    }
    assert_eq!(supply(&w), 0);
    assert_eq!(bal(&w.svm, &w.asset_reserve()), 0);
    assert_eq!(bal(&w.svm, &w.position_reserve()), 0);
}

#[test]
fn redeem_more_shares_than_owned_fails_token_insufficient_funds() {
    let (mut w, _, bob) = funded();
    // Bob has 50 shares and asks for 100: the Token program's burn refuses
    // (TokenError::InsufficientFunds = Custom(1)).
    let ix = ix_redeem(&w, bob.pubkey(), 100 * UNIT);
    assert_error(send(&mut w.svm, &[&bob], ix), 1);
}

#[test]
fn redeem_zero_fails_6000() {
    let (mut w, alice, _) = funded();
    let ix = ix_redeem(&w, alice.pubkey(), 0);
    assert_error(send(&mut w.svm, &[&alice], ix), 6000);
}

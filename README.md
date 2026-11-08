# Turbin3 Week 2: SOL Vault + Token Escrow (Anchor 1.1.2, LiteSVM)

One Anchor workspace with two programs:

| Program | What it does | Program ID |
|---|---|---|
| `q3_26_vault` | Each user gets a personal SOL vault: initialize, deposit, withdraw, close | `FJ3RsTGHZix7EyB9Mm4VxeTrumTxxfF1xzzkYpUk597X` |
| `escrow` | Trustless token swap: make, take, refund, update, with a deadline (Clock sysvar), an optional designated taker, and refund by anyone after the deadline | `9HVLYbuu3hYnvDxtvcww62Ubsd7AJbMKab4wR3rRDq6J` |

Built on the class vault starter [moses7054/anchor_vault_starter_q3_26](https://github.com/moses7054/anchor_vault_starter_q3_26) for the Turbin3 Q4 2026 Builders Cohort. The escrow was written from scratch, using [belivenn/anchor-escrow-q2-2026](https://github.com/belivenn/anchor-escrow-q2-2026) as a pattern reference.

**30 tests pass** (12 vault, 18 escrow). The full output is in [`test-output.txt`](test-output.txt).

**Why one workspace:** both programs share `anchor-lang` and `litesvm`, so the dependencies compile once into one `target/` folder. That matters on a low-memory machine.

## Run it

```bash
# Rust 1.89.0 (pinned in rust-toolchain.toml), Anchor CLI 1.1.2, Solana CLI 3.0.13
anchor build              # builds both .so files into target/deploy/
cargo test                # runs every LiteSVM test against those .so files
cargo test -p escrow      # one program only
```

The tests load the compiled `.so` from `target/deploy/`, so run `anchor build` first.

---

## 1. Vault

### Accounts and seeds

| Account | Type / owner | Seeds | Holds |
|---|---|---|---|
| `vault_state` | `Account<VaultState>`, owned by the vault program | `["state", user]` | `vault_bump`, `state_bump` (8-byte discriminator + 2 bytes = 10 bytes) |
| `vault` | `SystemAccount`, owned by the System Program | `["vault", user]` | the user's SOL, no data |

The user's key is a seed of both PDAs. So each user has exactly one vault, and nobody else can derive or pass it: the address would not match (error 2006 `ConstraintSeeds`).

### Instructions

| Instruction | Signer | What happens | Checks |
|---|---|---|---|
| `initialize` | user | creates `vault_state` (`init`), stores both bumps, sends the vault its rent-exempt minimum | `init` fails if it already exists |
| `deposit(amount)` | user | System `transfer` user to vault | `amount > 0` (6000) |
| `withdraw(amount)` | user | System `transfer` vault to user, signed by the vault PDA with `[b"vault", user, vault_bump]` via `CpiContext::new_with_signer` | `amount > 0` (6000); enough lamports (6001); after the withdraw the vault holds 0 or at least the rent minimum (6002) |
| `close` | user | the handler empties the vault with a signed transfer of all its lamports; then `close = user` closes `vault_state` | seeds |

**Why the vault needs a signed CPI:** the vault is owned by the System Program, so our program cannot debit it directly. Only the System Program can move its lamports, and only with the vault's signature. A PDA has no private key, so the program signs by passing the seeds and bump.

**Why `close = user` works on `vault_state` but not on `vault`:** `close` runs in `exit()` after the handler. It moves all lamports to `user`, sets the account's lamports to 0, assigns it to the System Program and resizes it to 0 bytes. It debits lamports directly, which is only allowed for accounts the program owns.

### State over time (LiteSVM rent, see NOTES)

| After | `vault_state` | `vault` |
|---|---|---|
| initialize | 10 bytes, 960,480 lamports | 0 bytes, 890,880 lamports (rent minimum) |
| deposit 0.5 SOL | same | 500,890,880 |
| withdraw 0.1 SOL | same | 400,890,880 |
| close | gone | gone; user received 400,890,880 + 960,480 |

---

## 2. Escrow

The maker deposits token A and asks for an amount of token B. Anyone (or one named taker) can accept before the deadline: they pay B to the maker and receive the A. The maker can cancel at any time and get A back; after the deadline anyone can trigger that refund, and the tokens still go to the maker.

### Accounts and seeds

| Account | Type | Seeds / address | Notes |
|---|---|---|---|
| `escrow` | `Account<Escrow>`, owned by the escrow program | `["escrow", maker, seed.to_le_bytes()]` | 162 bytes. `seed` lets one maker run many offers |
| `vault` | token account (ATA) | ATA of (`escrow`, `mint_a`) | authority is the escrow PDA, so only the program can move the tokens |
| `maker_ata_a`, `taker_ata_b` | existing ATAs | ATA rules | source accounts |
| `taker_ata_a`, `maker_ata_b` | `init_if_needed` ATAs | ATA rules | created in `take` if missing; the taker pays |

```rust
pub struct Escrow {
    pub seed: u64,
    pub maker: Pubkey,
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
    pub receive: u64,          // amount of mint_b wanted (updatable)
    pub deadline: i64,         // unix seconds; take is refused at or after it (updatable)
    pub taker: Option<Pubkey>, // if set, only this wallet may take
    pub bump: u8,
}
```

The token accounts use `token_interface` (`InterfaceAccount`, `Interface<TokenInterface>`), so the program works with both the Token and the Token-2022 programs. Every move uses `transfer_checked`, which also verifies the mint and decimals.

### Instructions

| Instruction | Signer | What happens | Checks and why |
|---|---|---|---|
| `make(seed, deposit, receive, deadline, taker)` | maker | creates `escrow` and its `vault`, moves `deposit` of A from the maker into the vault | `receive > 0`, `deposit > 0` (6000); `deadline` in the future (6001) |
| `take(expected_receive)` | taker | taker pays `receive` of B to the maker; the vault sends all A to the taker; the vault and escrow are closed and their rent goes to the maker | before the deadline (6002); designated taker only, if set (6003); `expected_receive == receive` (6004); `has_one` maker, mint_a, mint_b (2001); taker is not the maker (2040 from Anchor, 6005 as backup) |
| `refund` | maker any time; anyone after the deadline | the vault returns all A to the maker's ATA; the vault and escrow are closed and their rent goes to the maker; the caller only pays the fee | before the deadline a non-maker caller fails (6006); `maker` must be the stored maker (seeds 2006, `has_one`) and `maker_ata_a` must be the maker's ATA, so funds cannot be redirected |
| `update(new_receive, new_deadline)` | maker | changes the price and the deadline of an open offer | seeds and `has_one` (2006); offer still open (6002); `new_receive > 0` (6000); `new_deadline` in the future (6001) |

### What `update` may change, and who

- **Who:** only the maker. The escrow PDA is derived from the signer's key, and `has_one = maker` checks the stored maker.
- **When:** only while the offer is open (before the deadline). An expired offer can only be refunded.
- **What:** `receive` (the price) and `deadline`. Both are pure state, and no tokens move.
- **Not updatable:** `mint_a` and the deposit (tokens would have to move in or out of the vault, so use refund + make), `mint_b` (that changes the whole deal), and the designated taker.
- **Front-running guard:** a maker could raise the price while a taker's transaction is in flight. So `take` takes the price the taker saw (`expected_receive`) and fails with 6004 `TermsChanged` if it differs.

### Timed escrow (extension, task 4)

- **Deadline:** `Clock::get()?.unix_timestamp < deadline` in `take` and `update`. The tests move time with `svm.set_sysvar::<Clock>()`.
- **Signature condition:** the optional `taker: Option<Pubkey>`. If it is set, `take` requires that exact wallet's signature (6003).
- **Refund after the deadline is permissionless:** anyone (a keeper, the taker) can close an expired offer. Tokens and rent always go to the maker, so an expired offer is never stuck, even if the maker is offline. Before the deadline only the maker can cancel (6006).

### State over time

| After | `escrow` | `vault` (A) | maker A / B | taker A / B |
|---|---|---|---|---|
| make (100 A for 50 B) | 162 B, terms stored | 100 | 900 / 0 | 0 / 1000 |
| update (price 80 B, +6 days) | `receive` and `deadline` changed | 100 | same | same |
| take | closed, rent to maker | closed, rent to maker | 900 / 80 | 100 / 920 |
| or refund | closed | closed | 1000 / 0 | 0 / 1000 |

### Error codes

| Code | Name | Raised by |
|---|---|---|
| 6000 | InvalidAmount | make, update |
| 6001 | InvalidDeadline | make, update |
| 6002 | EscrowExpired | take, update |
| 6003 | NotDesignatedTaker | take |
| 6004 | TermsChanged | take |
| 6005 | MakerCannotTake | take (backup to Anchor's 2040) |
| 6006 | OnlyMakerBeforeDeadline | refund by a non-maker before the deadline |
| 2001 | ConstraintHasOne | wrong mint or maker passed |
| 2006 | ConstraintSeeds | someone other than the maker signs update, or passes a fake maker to refund |
| 2040 | ConstraintDuplicateMutableAccount | maker tries to take their own offer |

---

## Tests (LiteSVM, Rust)

Every instruction has a success test with exact balance assertions, and every failure test asserts the exact error number.

| Vault (12) | Escrow (18) |
|---|---|
| full lifecycle with exact lamports; withdraw down to the rent minimum; withdraw the full balance to 0, then close; re-initialize after close; zero deposit/withdraw (6000); withdraw more than the balance (6001); withdraw that leaves dust below rent (6002); another user's withdraw or close (2006); initialize twice (System error 0); withdraw after close (3012) | make stores terms and locks tokens; take swaps and closes; refund returns and closes; refund by the maker after the deadline; anyone refunds after the deadline and the maker gets tokens and rent; refund by a non-maker before the deadline (6006); fake maker on refund (2006); update then take at the new price after the old deadline; past deadline on make (6001); zero amounts (6000); take after the deadline (6002); designated taker (6003, then success); stale price (6004); wrong mint_b (2001); maker takes own offer (2040); update by someone else (2006); update after the deadline (6002); bad update values (6000, 6001) |

## Security notes

- No keypair is committed (`target/` is gitignored and holds the program keypairs).
- Not audited; localnet tests only.

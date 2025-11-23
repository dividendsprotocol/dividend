# Dividends Protocol

Dividends is a Solana-native, open source protocol that enables tokenized companies to offer on-chain dividends to their holders. This repository contains the Anchor program that powers vault-based staking and automated payouts for SPL tokens on Solana mainnet.

Website: https://dividends.run  
Chain: Solana (mainnet)  
Program id (in `src/lib.rs`): `FZ7rgdWAfFDHZKV4NYRkxDgZiXUPe5rEqZ3vCk5VKu6Z`

## What the on-chain program does
- Initializes a dividend vault for a specific SPL mint with configurable base rate and payout period (in minutes).
- Lets holders stake into the vault across up to five slots per wallet, tracking deposit time and stake time per slot.
- Calculates rewards on withdraw using a compounding step function that scales with both time held and current vault size vs. the initial pool.
- Supports withdrawing only rewards (keeping principal staked) or withdrawing the full stake plus rewards.
- Tracks total and current stakers to give projects basic participation telemetry.

## Instruction flow
- `create_corn_vault(amount, base_rate, base_minutes)`: PDA-seeded vault and token vault are created for the mint; the creator funds the vault token account with the initial pool. Guards: amount > 0 and only one vault per mint.
- `deposit_corn(amount, index)`: A depositor stakes into one of five indexed slots (0–4). Guards: amount between `100_000_000` and `1_000_000_000_000_000_000`, one active stake per slot, depositor has funds. First-time stakers increment the vault’s staker counters.
- `withdraw_corn(index, reward_only)`: Withdraws from a slot. Rewards are computed from `calculate_reward` and limited by vault balance; `reward_only` either pays just the gain or the full stake plus gain. Fully exiting all slots decrements current stakers.

## Reward model
- Time-based: rewards accrue after the configured `base_minutes` and compound every `base_minutes` thereafter.
- Pool-aware: the per-period multiplier is `(base_rate * (current_vault_amount / start_pool)) / 100`, so payouts scale with available liquidity.
- Safety checks: returns zero when below the minimum hold time; rejects when vault lacks funds; prevents overflows.

## Accounts and PDAs
- Vault: `seeds = ["vault", mint]` stores mint, total pool, staked total, starting pool, rate, period, and staker counts.
- Vault token account: `seeds = ["token_vault", mint]` holds staked tokens and pays withdrawals; acts as the signer for outbound transfers.
- User interactions: `seeds = ["interactor", user, mint]` tracks up to five deposit amounts plus timestamps (deposit time and stake time) per user.

## Development
- Tooling: Anchor 0.29.x, Rust stable, Solana CLI compatible with your target cluster. Ensure your `Anchor.toml` program id matches `src/lib.rs`.
- Build: `anchor build`
- Lint: `cargo fmt && cargo clippy`
- Test: no tests are included yet; add Anchor integration tests before mainnet deployments.

## Security
- The `security_txt!` block currently references Corn Protocol details; update it to Dividends contacts and policy before production use.
- Perform your own audits and simulations; the reward math is sensitive to `base_rate`, `base_minutes`, and vault liquidity. Start with small amounts on a devnet fork before mainnet rollout.

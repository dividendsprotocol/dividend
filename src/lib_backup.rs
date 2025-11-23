use anchor_lang::prelude::*;
use anchor_lang::solana_program::pubkey::Pubkey;
use anchor_spl::token_interface::{ TokenInterface, Mint };
use anchor_spl::token::{ Token, Transfer, TokenAccount };
use anchor_spl::token::transfer;
use borsh::BorshDeserialize;
use std::convert::{ TryInto, TryFrom };
#[allow(unused_imports)]
use solana_security_txt::security_txt;

declare_id!("5bqRVimADKTP3gyRGzA1WnAHmKvdTSHJRNs8PWh7tCbs");

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Corn Protocol",
    project_url: "http://corn-protocol.fun/",
    contacts: "email:contact@corn-protocol.fun, twitter:@Corn_Protocol, link:http://corn-protocol.fun/",
    policy: "https://github.com/CornProtocol/Corn_Protocol/blob/main/README.md",
    preferred_languages: "en",
    source_code: "https://github.com/CornProtocol/Corn_Protocol"
}

fn calculate_reward(
    amount: u64,
    time_elapsed_ms: u64,
    base_hour: u64,
    base_rate: f32,
    vault_amount: u64,
    start_pool: u64
) -> Option<u64> {
    if vault_amount == 0 || start_pool == 0 || base_hour == 0 {
        return None;
    }

    let held_hours = (time_elapsed_ms / 3_600_000).min(24);
    if held_hours < base_hour {
        return Some(0);
    }

    let pool_percent = (vault_amount as f64) / (start_pool as f64);
    let mut current_amount = amount as f64;

    for hour in 1..=held_hours {
        if hour % base_hour == 0 {
            let step_multiplier = ((base_rate as f64) * pool_percent) / 100.0;
            current_amount *= 1.0 + step_multiplier;
        }
    }

    Some(current_amount.floor() as u64)
}

#[program]
pub mod corn_vault {
    use super::*;

    pub fn create_corn_vault(
        ctx: Context<CreateVault>,
        amount: u64,
        base_rate: f32,
        base_hour: u32
    ) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        let creator_token_account = &ctx.accounts.creator_token_account;

        require!(amount > 0, CornError::InvalidAmount);
        require!(vault.amount == 0, CornError::AlreadyExists);
        require!(creator_token_account.amount >= amount, CornError::InsufficientFunds);

        vault.token = ctx.accounts.mint.key();
        vault.amount = amount;
        vault.amount_staked = 0;
        vault.start_pool = amount;
        vault.base_rate = base_rate;
        vault.base_hour = base_hour;
        vault.total_stakers = 0;
        vault.current_stakers = 0;

        let cpi_accounts = Transfer {
            from: creator_token_account.to_account_info(),
            to: ctx.accounts.token_account.to_account_info(),
            authority: ctx.accounts.creator.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();

        transfer(CpiContext::new(cpi_program, cpi_accounts), amount)?;

        Ok(())
    }

    pub fn deposit_corn(ctx: Context<Deposit>, amount: u64, index: u32) -> Result<()> {
        let clock = Clock::get()?;
        let user_counter = &mut ctx.accounts.user_interactions_counter;
        let vault = &mut ctx.accounts.vault;
        let depositor_token_account = &ctx.accounts.depositor_token_account;

        require!((100_000_000..=1_000_000_000_000_000_000).contains(&amount), CornError::InvalidAmount);
        require!(index <= 4, CornError::OutOfRange);
        require!(depositor_token_account.amount >= amount, CornError::InsufficientFunds);

        let index = usize::try_from(index).map_err(|_| CornError::OutOfRange)?;
        require!(user_counter.total_deposits[index] == 0, CornError::AlreadyStaked);
        // Check if this is the user's first stake (all slots were 0 before this)
        let is_first_stake = user_counter.total_deposits.iter().all(|&x| x == 0);
        if is_first_stake {
            vault.total_stakers += 1;
            vault.current_stakers += 1;
        }

        let timestamp = clock.unix_timestamp as u64;
        user_counter.total_deposits[index] = amount;
        user_counter.time_deposits[index] = timestamp;
        user_counter.stake_deposits[index] = timestamp;

        vault.amount_staked += amount;

        let cpi_accounts = Transfer {
            from: depositor_token_account.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.depositor.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        transfer(CpiContext::new(cpi_program, cpi_accounts), amount)?;

        Ok(())
    }

    pub fn withdraw_corn(ctx: Context<Withdraw>, index: u32, reward_only: bool) -> Result<()> {
        let clock = Clock::get()?;
        let now: u64 = clock.unix_timestamp.try_into().map_err(|_| CornError::TimeConversionError)?;

        let user_counter = &mut ctx.accounts.user_interactions_counter;
        let vault = &mut ctx.accounts.vault;
        let vault_token_account = &ctx.accounts.vault_token_account;

        let index = usize::try_from(index).map_err(|_| CornError::OutOfRange)?;
        require!(index <= 4, CornError::OutOfRange);

        let amount = user_counter.total_deposits[index];
        require!(amount > 0, CornError::NoDeposits);

        let seed = ctx.accounts.mint.key();
        let bump_seed = ctx.bumps.vault_token_account;
        let signer_seeds: &[&[&[u8]]] = &[&[b"token_vault", seed.as_ref(), &[bump_seed]]];

        let stake_time = user_counter.stake_deposits[index];
        let time_elapsed = now.saturating_sub(stake_time).saturating_mul(1_000);

        let mut withdraw_amount = amount;

        if
            let Some(reward) = calculate_reward(
                amount,
                time_elapsed,
                u64::from(vault.base_hour),
                vault.base_rate,
                vault.amount,
                vault.start_pool
            )
        {
            if reward > 0 {
                require!(vault.amount >= reward, CornError::EmptyVault);
                let gain = reward.checked_sub(withdraw_amount).ok_or(CornError::MathOverflow)?;
                vault.amount = vault.amount.checked_sub(gain).ok_or(CornError::MathOverflow)?;

                withdraw_amount = if reward_only {
                    gain
                } else {
                    reward
                };
            } else if reward_only {
                withdraw_amount = 0;
            }
        }

        if reward_only {
            user_counter.stake_deposits[index] = now;
        } else {
            user_counter.total_deposits[index] = 0;
            user_counter.time_deposits[index] = 0;
            user_counter.stake_deposits[index] = 0;

            vault.amount_staked = vault.amount_staked
                .checked_sub(amount)
                .ok_or(CornError::MathOverflow)?;
        }

        // Check if this withdrawal makes all slots 0 (user fully exits)
        let all_slots_empty = user_counter.total_deposits.iter().all(|&x| x == 0);
        if all_slots_empty {
            vault.current_stakers = vault.current_stakers.saturating_sub(1);
        }
        if withdraw_amount > 0 {
            let cpi_accounts = Transfer {
                from: vault_token_account.to_account_info(),
                to: ctx.accounts.withdrawer_token_account.to_account_info(),
                authority: vault_token_account.to_account_info(),
            };
            let cpi_program = ctx.accounts.token_program.to_account_info();

            transfer(
                CpiContext::new(cpi_program, cpi_accounts).with_signer(signer_seeds),
                withdraw_amount
            )?;
        }

        Ok(())
    }
}

#[derive(Accounts)]
pub struct CreateVault<'info> {
    #[account(init, payer = creator, space = 160, seeds = [b"vault", mint.key().as_ref()], bump)]
    pub vault: Account<'info, Vault>,
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(
        init,
        payer = creator,
        token::mint = mint,
        token::authority = token_account,
        token::token_program = token_program,
        seeds = [b"token_vault", mint.key().as_ref()],
        bump
    )]
    pub token_account: Account<'info, TokenAccount>,
    #[account(mut, token::authority = creator.key(), token::mint = mint.key())]
    pub creator_token_account: Account<'info, TokenAccount>,
    pub mint: InterfaceAccount<'info, Mint>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut, seeds = [b"vault", mint.key().as_ref()], bump)]
    pub vault: Account<'info, Vault>,
    #[account(mut)]
    pub depositor: Signer<'info>,
    #[account(mut, token::authority = depositor.key(), token::mint = mint.key())]
    pub depositor_token_account: Account<'info, TokenAccount>,
    #[account(mut, token::mint = mint,
        token::authority = vault_token_account,
        token::token_program = token_program,
        seeds = [b"token_vault", mint.key().as_ref()], bump)]
    pub vault_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(
        init_if_needed,
        space = 128,
        seeds = [b"interactor", depositor.key().as_ref(), mint.key().as_ref()],
        bump,
        payer = depositor
    )]
    pub user_interactions_counter: Account<'info, UserInteractions>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut, seeds = [b"vault", mint.key().as_ref()], bump)]
    pub vault: Account<'info, Vault>,
    #[account(mut)]
    pub withdrawer: Signer<'info>,
    #[account(mut, token::authority = withdrawer.key(), token::mint = mint.key())]
    pub withdrawer_token_account: Account<'info, TokenAccount>,
    #[account(mut, seeds = [b"token_vault", mint.key().as_ref()], bump)]
    pub vault_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(
        mut,
        seeds = [b"interactor", withdrawer.key().as_ref(), mint.key().as_ref()],
        bump,
    )]
    pub user_interactions_counter: Account<'info, UserInteractions>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct Vault {
    pub token: Pubkey,
    pub amount: u64,
    pub amount_staked: u64,
    pub start_pool: u64,
    pub base_rate: f32,
    pub base_hour: u32,
    pub total_stakers: u64,
    pub current_stakers: u64,
}

#[account]
pub struct UserInteractions {
    total_deposits: [u64; 5],
    time_deposits: [u64; 5],
    stake_deposits: [u64; 5],
}

#[error_code]
pub enum CornError {
    #[msg("No corn staked")]
    NoDeposits,
    #[msg("Corn amount out of range")]
    InvalidAmount,
    #[msg("Corn stake index out of range")]
    OutOfRange,
    #[msg("Corn vault already initialized")]
    AlreadyExists,
    #[msg("Not enough corn to deposit")]
    InsufficientFunds,
    #[msg("Account already has an active stake")]
    AlreadyStaked,
    #[msg("Vault is empty")]
    EmptyVault,
    #[msg("Invalid timestamp")]
    TimeConversionError,
    #[msg("Math overflow")]
    MathOverflow,
}

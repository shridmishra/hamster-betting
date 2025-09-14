use anchor_lang::prelude::*;

declare_id!("5nyLrZamcFzJNqKDxtNFKBaYD5XWTAx5UDFaLuZsZagN");

#[program]
pub mod hamster_betting {
    use super::*;

    pub fn create_race(
        ctx: Context<CreateRace>,
        title: String,
        livestream: String,
        hamsters: Vec<String>,
    ) -> Result<()> {
        let race = &mut ctx.accounts.race;
        race.admin = ctx.accounts.admin.key();
        race.title = title;
        race.livestream = livestream;
        race.hamsters = hamsters.clone();
        race.status = 0; // upcoming
        race.winner_index = None;
        race.total_pool = 0;
        race.hamster_pools = vec![0; hamsters.len()];
        Ok(())
    }

    pub fn stop_betting(ctx: Context<StopBetting>) -> Result<()> {
        let race = &mut ctx.accounts.race;
        require_keys_eq!(
            race.admin,
            ctx.accounts.admin.key(),
            BettingError::Unauthorized
        );
        require!(race.status == 0, BettingError::RaceNotUpcoming);
        race.status = 1; // live
        Ok(())
    }

    pub fn place_bet(ctx: Context<PlaceBet>, hamster_index: u8, amount: u64) -> Result<()> {
        let bet = &mut ctx.accounts.bet;

        // Perform checks and mutable updates in a separate scope
        {
            let race = &mut ctx.accounts.race;
            require!(
                (hamster_index as usize) < race.hamsters.len(),
                BettingError::InvalidHamster
            );
            require!(
                race.status == 0 || race.status == 1,
                BettingError::RaceClosed
            );
        }

        // Transfer lamports into vault PDA
        let ix = anchor_lang::solana_program::system_instruction::transfer(
            &ctx.accounts.bettor.key(),
            &ctx.accounts.vault.key(),
            amount,
        );
        anchor_lang::solana_program::program::invoke(
            &ix,
            &[
                ctx.accounts.bettor.to_account_info(),
                ctx.accounts.vault.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;

        bet.bettor = ctx.accounts.bettor.key();
        bet.race = ctx.accounts.race.key();
        bet.hamster_index = hamster_index;
        bet.amount = amount;
        bet.claimed = false;

        // Update race pools in another mutable borrow
        {
            let race = &mut ctx.accounts.race;
            race.total_pool = race
                .total_pool
                .checked_add(amount)
                .ok_or(BettingError::Overflow)?;
            race.hamster_pools[hamster_index as usize] = race.hamster_pools[hamster_index as usize]
                .checked_add(amount)
                .ok_or(BettingError::Overflow)?;
        }

        Ok(())
    }
    pub fn set_winner(ctx: Context<SetWinner>, winner_index: u8) -> Result<()> {
        let race = &mut ctx.accounts.race;
        require_keys_eq!(
            race.admin,
            ctx.accounts.admin.key(),
            BettingError::Unauthorized
        );
        require!(
            (winner_index as usize) < race.hamsters.len(),
            BettingError::InvalidHamster
        );

        race.status = 2; // finished
        race.winner_index = Some(winner_index);
        Ok(())
    }

    pub fn claim_winnings(ctx: Context<ClaimWinnings>) -> Result<()> {
        let race = &mut ctx.accounts.race;
        let bet = &mut ctx.accounts.bet;

        require!(race.status == 2, BettingError::RaceNotFinished);
        require!(!bet.claimed, BettingError::AlreadyClaimed);

        let winner_index = race.winner_index.ok_or(BettingError::WinnerNotSet)?;
        require!(bet.hamster_index == winner_index, BettingError::NotWinner);

        let total_pool = race.total_pool;
        let winner_pool = race.hamster_pools[winner_index as usize];
        require!(winner_pool > 0, BettingError::MathError);

        let payout = (bet.amount as u128)
            .checked_mul(total_pool as u128)
            .ok_or(BettingError::Overflow)?
            .checked_div(winner_pool as u128)
            .ok_or(BettingError::MathError)? as u64;

        **ctx
            .accounts
            .vault
            .to_account_info()
            .try_borrow_mut_lamports()? -= payout;
        **ctx
            .accounts
            .bettor
            .to_account_info()
            .try_borrow_mut_lamports()? += payout;

        bet.claimed = true;
        Ok(())
    }
}

#[account]
pub struct Race {
    pub admin: Pubkey,
    pub title: String,
    pub livestream: String,
    pub hamsters: Vec<String>,
    pub status: u8, // 0 = upcoming, 1 = live, 2 = finished
    pub winner_index: Option<u8>,
    pub total_pool: u64,
    pub hamster_pools: Vec<u64>,
}

#[account]
pub struct Bet {
    pub bettor: Pubkey,
    pub race: Pubkey,
    pub hamster_index: u8,
    pub amount: u64,
    pub claimed: bool,
}

#[derive(Accounts)]
pub struct CreateRace<'info> {
    #[account(init, payer = admin, space = 8 + 8192)]
    pub race: Account<'info, Race>,
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        init,
        payer = admin,
        seeds = [b"vault", race.key().as_ref()],
        bump,
        space = 8
    )]
    /// CHECK: vault PDA (stores lamports only)
    pub vault: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct StopBetting<'info> {
    #[account(mut)]
    pub race: Account<'info, Race>,
    pub admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct PlaceBet<'info> {
    #[account(mut)]
    pub race: Account<'info, Race>,
    #[account(init, payer = bettor, space = 8 + 256)]
    pub bet: Account<'info, Bet>,
    #[account(mut)]
    pub bettor: Signer<'info>,
    #[account(
        mut,
        seeds = [b"vault", race.key().as_ref()],
        bump
    )]
    /// CHECK: vault PDA
    pub vault: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SetWinner<'info> {
    #[account(mut)]
    pub race: Account<'info, Race>,
    pub admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct ClaimWinnings<'info> {
    #[account(mut)]
    pub race: Account<'info, Race>,
    #[account(mut, has_one = bettor)]
    pub bet: Account<'info, Bet>,
    #[account(mut)]
    pub bettor: Signer<'info>,
    #[account(
        mut,
        seeds = [b"vault", race.key().as_ref()],
        bump
    )]
    /// CHECK: vault PDA
    pub vault: UncheckedAccount<'info>,
}

#[error_code]
pub enum BettingError {
    #[msg("You are not authorized to perform this action.")]
    Unauthorized,
    #[msg("Race not finished yet.")]
    RaceNotFinished,
    #[msg("Winner not set yet.")]
    WinnerNotSet,
    #[msg("This bet already claimed.")]
    AlreadyClaimed,
    #[msg("Your hamster did not win.")]
    NotWinner,
    #[msg("Invalid hamster index.")]
    InvalidHamster,
    #[msg("Math error.")]
    MathError,
    #[msg("Overflow occurred.")]
    Overflow,
    #[msg("Race is closed for betting.")]
    RaceClosed,
    #[msg("Race is not in upcoming state.")]
    RaceNotUpcoming,
}

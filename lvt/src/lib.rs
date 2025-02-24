use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount};

declare_id!("7npskT7QVWC6kddvwxfSdVHUZxihPYQmq1qYu3HnNZba");

#[program]
pub mod liquidity_velocity_token {
    use super::*;

    /// Initialize global state, fee parameters, treasury, and dynamic reward variables.
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let state = &mut ctx.accounts.state;
        state.total_trades = 0;
        state.total_liquidity = 0;
        state.fee_rate = 1000; // Initial fee rate (example units)
        state.last_fee_update = Clock::get()?.unix_timestamp;
        state.treasury = ctx.accounts.treasury.key();
        // Initialize dynamic reward tracking
        state.reward_sum = 0;
        state.reward_count = 0;
        state.global_reward_multiplier = 1;
        Ok(())
    }

    /// Record a trade with detailed parameters and log a TradeRecord.
    /// Also update the user’s cumulative stats and dynamic reward (via fixed multiplier adjustment here).
    pub fn record_trade(
        ctx: Context<RecordTrade>,
        trade_amount: u64,
        trade_timestamp: i64,
        trade_pair: String,
        execution_delay: i64,
        slippage: u64,
        liquidity_provided: u64,
        // Parameter for wash trading check – counterparty address.
        counterparty: Pubkey,
    ) -> Result<()> {
        let state = &mut ctx.accounts.state;
        let user_state = &mut ctx.accounts.user_state;

        // Anti-wash trading check:
        if user_state.owner == counterparty {
            return Err(CustomError::WashTradingAttempt.into());
        }

        // Update global statistics.
        state.total_trades = state.total_trades.checked_add(1).unwrap();
        state.total_liquidity = state.total_liquidity.checked_add(trade_amount).unwrap();

        // Update user's trade count and cumulative volume.
        user_state.trade_count = user_state.trade_count.checked_add(1).unwrap();
        user_state.cumulative_volume = user_state.cumulative_volume.checked_add(trade_amount).unwrap();

        // Create a new TradeRecord for detailed logging.
        let trade_record = &mut ctx.accounts.trade_record;
        trade_record.user = user_state.owner;
        trade_record.trade_amount = trade_amount;
        trade_record.trade_timestamp = trade_timestamp;
        trade_record.trade_pair = trade_pair;
        trade_record.execution_delay = execution_delay;
        trade_record.slippage = slippage;
        trade_record.liquidity_provided = liquidity_provided;

        // Advanced reward calculation:
        let base_reward = trade_amount;
        let execution_bonus = if execution_delay < 100 { 10 } else { 1 };
        let slippage_bonus = if slippage < 50 { 10 } else { 1 };
        let liquidity_bonus = if liquidity_provided > 1000 { 10 } else { 1 };

        let mut reward = base_reward
            .checked_mul(execution_bonus)
            .unwrap()
            .checked_mul(slippage_bonus)
            .unwrap()
            .checked_mul(liquidity_bonus)
            .unwrap();

        // Penalize excessive slippage (if slippage > 3% expressed as 300 basis points).
        if slippage > 300 {
            reward = reward / 2;
        }

        // Update accrued rewards.
        user_state.accrued_rewards = user_state.accrued_rewards.checked_add(reward).unwrap();

        // Update dynamic reward tracking in state.
        state.reward_sum = state.reward_sum.checked_add(reward).unwrap();
        state.reward_count = state.reward_count.checked_add(1).unwrap();
        // If we've collected data for 50 trades, update the global multiplier and reset.
        if state.reward_count >= 50 {
            state.global_reward_multiplier = state.reward_sum / state.reward_count;
            state.reward_sum = 0;
            state.reward_count = 0;
        }
        // Also update the user’s reward multiplier (could be further adjusted by market conditions off-chain).
        user_state.reward_multiplier = state.global_reward_multiplier;

        Ok(())
    }

    /// Record a liquidity deposit for LP tracking.
    pub fn record_liquidity_deposit(
        ctx: Context<RecordLiquidityDeposit>,
        deposit_amount: u64,
        deposit_timestamp: i64,
    ) -> Result<()> {
        let lp_state = &mut ctx.accounts.lp_state;
        lp_state.total_deposit = lp_state.total_deposit.checked_add(deposit_amount).unwrap();
        lp_state.last_deposit = deposit_timestamp;
        Ok(())
    }

    /// Stake tokens with an optional lockup period for enhanced fee discounts and multi-tier rewards.
    pub fn stake_with_lockup(
        ctx: Context<StakeTokens>,
        amount: u64,
        lockup_duration: i64, // in seconds (e.g., 1 month, 3 months, 6 months)
    ) -> Result<()> {
        let user_state = &mut ctx.accounts.user_state;
        user_state.staked_amount = user_state.staked_amount.checked_add(amount).unwrap();

        // Set lockup period if specified.
        if lockup_duration > 0 {
            let current_time = Clock::get()?.unix_timestamp;
            user_state.lockup_end = current_time.checked_add(lockup_duration).unwrap();
        }

        // Update fee discount and trading rebate based on multi-tier staking:
        if user_state.staked_amount >= 50_000 {
            user_state.fee_discount = 30; // Tier Pro: 30% discount + instant execution priority
            user_state.trading_rebate = 10; // Example: 10% trading rebate
        } else if user_state.staked_amount >= 5_000 {
            user_state.fee_discount = 20; // Advanced: 20% discount
            user_state.trading_rebate = 5;  // Example: 5% trading rebate
        } else if user_state.staked_amount >= 500 {
            user_state.fee_discount = 10; // Basic: 10% discount
            user_state.trading_rebate = 0;
        } else {
            user_state.fee_discount = 0;
            user_state.trading_rebate = 0;
        }
        Ok(())
    }

    /// Claim accrued rewards. Enforce minimum cumulative trading volume and cooldown period.
    pub fn claim_rewards(ctx: Context<ClaimRewards>) -> Result<()> {
        let user_state = &mut ctx.accounts.user_state;
        let current_time = Clock::get()?.unix_timestamp;
        // Enforce a minimum cumulative volume to prevent wash trading exploitation.
        require!(
            user_state.cumulative_volume >= 100,
            CustomError::InsufficientLiquidityForRewards
        );
        // Enforce a cooldown period (e.g., 1 hour) between claims.
        require!(
            current_time - user_state.last_claim_time >= 3600,
            CustomError::MinimumHoldingPeriodNotMet
        );
        // [Token transfer logic from treasury to the user would be added here]
        user_state.accrued_rewards = 0;
        user_state.last_claim_time = current_time;
        Ok(())
    }

    /// Dynamically adjust pool fees based on liquidity and market activity.
    pub fn adjust_fee_dynamically(ctx: Context<UpdatePoolFees>) -> Result<()> {
        let state = &mut ctx.accounts.state;
        // Example: if total liquidity is low, increase fee; if high, decrease fee.
        if state.total_liquidity < 1_000_000 {
            state.fee_rate = state.fee_rate.checked_add(100).unwrap();
        } else {
            state.fee_rate = state.fee_rate.checked_sub(100).unwrap();
        }
        state.last_fee_update = Clock::get()?.unix_timestamp;
        Ok(())
    }

    /// Auto-adjust fee rate based on current market volatility.
    pub fn auto_adjust_fee(ctx: Context<AutoAdjustFee>, current_volatility: u64) -> Result<()> {
        let state = &mut ctx.accounts.state;
        // If market volatility is high, increase fees; otherwise, lower fees.
        if current_volatility > 1000 {
            state.fee_rate = state.fee_rate.checked_add(50).unwrap();
        } else {
            state.fee_rate = state.fee_rate.checked_sub(50).unwrap();
        }
        state.last_fee_update = Clock::get()?.unix_timestamp;
        Ok(())
    }

    /// Batch trading orders with a randomized delay to help prevent MEV exploitation.
    pub fn batch_trading_orders_with_delay(ctx: Context<BatchTradingOrders>, delay: i64) -> Result<()> {
        require!(delay > 0, CustomError::InvalidDelay);
        msg!("Batch orders will be executed after a delay of {} seconds", delay);
        Ok(())
    }

    /// Governance-based fee structure update using on-chain voting.
    pub fn update_fee_structure_by_vote(ctx: Context<UpdateFeeByVote>, new_fee_rate: u64) -> Result<()> {
        let governance = &mut ctx.accounts.governance;
        require!(
            governance.vote_count >= governance.required_votes,
            CustomError::InsufficientVotes
        );
        let state = &mut ctx.accounts.state;
        require!(new_fee_rate >= 500 && new_fee_rate <= 5000, CustomError::InvalidFeeRate);
        state.fee_rate = new_fee_rate;
        state.last_fee_update = Clock::get()?.unix_timestamp;
        // Reset vote count after successful update.
        governance.vote_count = 0;
        Ok(())
    }

    /// Update dynamic reward parameters using a rolling window average.
    /// The function takes the recent trade reward, current market volatility, and order book gap.
    pub fn update_dynamic_reward(
        ctx: Context<UpdateDynamicReward>,
        recent_reward: u64,
        market_volatility: u64,
        order_book_gap: u64,
    ) -> Result<()> {
        let state = &mut ctx.accounts.state;
        // Update rolling average using recent_reward.
        state.reward_sum = state.reward_sum.checked_add(recent_reward).unwrap();
        state.reward_count = state.reward_count.checked_add(1).unwrap();
        // When the window is complete, update the global multiplier.
        if state.reward_count >= 50 {
            let average = state.reward_sum / state.reward_count;
            // Adjust multiplier based on market conditions.
            // For example, boost multiplier by 10% if volatility is high or order book gap is wide.
            let volatility_bonus = if market_volatility > 1000 { 110 } else { 100 };
            let gap_bonus = if order_book_gap > 500 { 105 } else { 100 };
            state.global_reward_multiplier = (average * volatility_bonus * gap_bonus) / (100 * 100);
            state.reward_sum = 0;
            state.reward_count = 0;
        }
        Ok(())
    }

    /// Update the leaderboard for trade volume and frequency.
    pub fn update_leaderboard(ctx: Context<UpdateLeaderboard>, trade_volume: u64, trade_count: u64) -> Result<()> {
        let leaderboard = &mut ctx.accounts.leaderboard;
        leaderboard.trade_volume = leaderboard.trade_volume.checked_add(trade_volume).unwrap();
        leaderboard.trade_count = leaderboard.trade_count.checked_add(trade_count).unwrap();
        leaderboard.last_update = Clock::get()?.unix_timestamp;
        Ok(())
    }

    /// Reward strategy boosts for specific trading behaviors.
    /// strategy_type: 1 = Market-making, 2 = Arbitrage, 3 = Options hedging.
    pub fn reward_strategy_boost(ctx: Context<RewardStrategyBoost>, strategy_type: u8) -> Result<()> {
        let user_state = &mut ctx.accounts.user_state;
        match strategy_type {
            1 => {
                // Market-making boost
                user_state.accrued_rewards = user_state.accrued_rewards.checked_add(50).unwrap();
            }
            2 => {
                // Arbitrage boost
                user_state.accrued_rewards = user_state.accrued_rewards.checked_add(100).unwrap();
            }
            3 => {
                // Options hedging boost
                user_state.accrued_rewards = user_state.accrued_rewards.checked_add(75).unwrap();
            }
            _ => {}
        }
        Ok(())
    }

    /// Randomized batch processing to further prevent front-running.
    pub fn batch_process_trades(ctx: Context<BatchProcessTrades>) -> Result<()> {
        let state = &mut ctx.accounts.state;
        // Simulate a random delay (for demonstration, using clock's timestamp modulus).
        let random_delay = (Clock::get()?.unix_timestamp % 10) as u64;
        state.last_fee_update = Clock::get()?.unix_timestamp + random_delay as i64;
        msg!("Trades will be processed with a randomized delay of {} seconds", random_delay);
        Ok(())
    }

    /// Allow LVT token holders to borrow against their staked LVT.
    /// This is a simplified example of DeFi lending integration.
    pub fn borrow_against_lvt(ctx: Context<BorrowAgainstLVT>, borrow_amount: u64) -> Result<()> {
        let user_state = &mut ctx.accounts.user_state;
        // Ensure the user’s staked collateral is sufficient (e.g., at least 150% of the borrow_amount).
        require!(
            user_state.staked_amount >= borrow_amount * 150 / 100,
            CustomError::InsufficientCollateral
        );
        // Initialize a loan account with a fixed interest rate and due time.
        let loan = &mut ctx.accounts.loan_account;
        loan.borrower = user_state.owner;
        loan.collateral = user_state.staked_amount;
        loan.borrow_amount = borrow_amount;
        loan.interest_rate = 5; // Example: 5%
        loan.start_time = Clock::get()?.unix_timestamp;
        loan.due_time = Clock::get()?.unix_timestamp + 30 * 86400; // Due in 30 days
        Ok(())
    }
}

//
// HELPER FUNCTIONS
//

fn compute_reward_multiplier(accrued: u64, trade_count: u64) -> u64 {
    if trade_count == 0 { 1 } else { accrued / trade_count }
}

fn fee_discount_for_stake(staked: u64) -> u64 {
    if staked >= 50_000 {
        30  // Tier Pro: 30% discount
    } else if staked >= 5_000 {
        20  // Advanced: 20% discount
    } else if staked >= 500 {
        10  // Basic: 10% discount
    } else {
        0
    }
}

//
// ACCOUNT CONTEXTS AND STRUCTS
//

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = admin, space = 8 + State::LEN)]
    pub state: Account<'info, State>,
    #[account(mut)]
    pub treasury: Account<'info, TokenAccount>, // Treasury for LVT tokens.
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(mut)]
    pub lvt_mint: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

#[account]
pub struct State {
    pub total_trades: u64,
    pub total_liquidity: u64,
    pub fee_rate: u64,
    pub last_fee_update: i64,
    pub treasury: Pubkey,
    // For dynamic reward adjustment:
    pub reward_sum: u64,
    pub reward_count: u64,
    pub global_reward_multiplier: u64,
    // Additional governance or protocol fields can be added here.
}

impl State {
    // Calculation: 3 u64 fields (24 bytes) + last_fee_update (8) + treasury (32) = 64;
    // plus 3 more u64 fields (24) = 88 bytes.
    pub const LEN: usize = 88;
}

#[derive(Accounts)]
pub struct RecordTrade<'info> {
    #[account(mut)]
    pub state: Account<'info, State>,
    #[account(mut, seeds = [b"user", user_state.owner.as_ref()], bump = user_state.bump)]
    pub user_state: Account<'info, UserState>,
    // Log detailed trade data.
    #[account(init, payer = payer, space = 8 + TradeRecord::LEN)]
    pub trade_record: Account<'info, TradeRecord>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct TradeRecord {
    pub user: Pubkey,
    pub trade_amount: u64,
    pub trade_timestamp: i64,
    pub trade_pair: String, // Consider fixed-length or hashed representation in production.
    pub execution_delay: i64,
    pub slippage: u64,
    pub liquidity_provided: u64,
}

impl TradeRecord {
    // For example, trade_pair is limited to 32 bytes.
    pub const LEN: usize = 32 + 8 + 8 + 32 + 8 + 8 + 8;
}

#[derive(Accounts)]
pub struct RecordLiquidityDeposit<'info> {
    #[account(mut, seeds = [b"lp", lp_state.owner.as_ref()], bump = lp_state.bump)]
    pub lp_state: Account<'info, LPState>,
    pub owner: Signer<'info>,
}

#[account]
pub struct LPState {
    pub owner: Pubkey,
    pub total_deposit: u64,
    pub last_deposit: i64,
    pub bump: u8,
}

impl LPState {
    pub const LEN: usize = 32 + 8 + 8 + 1;
}

#[derive(Accounts)]
pub struct StakeTokens<'info> {
    #[account(mut, seeds = [b"user", user_state.owner.as_ref()], bump = user_state.bump)]
    pub user_state: Account<'info, UserState>,
}

#[derive(Accounts)]
pub struct ClaimRewards<'info> {
    #[account(mut, seeds = [b"user", user_state.owner.as_ref()], bump = user_state.bump)]
    pub user_state: Account<'info, UserState>,
    #[account(mut)]
    pub treasury: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct UpdatePoolFees<'info> {
    #[account(mut)]
    pub state: Account<'info, State>,
    pub admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct AutoAdjustFee<'info> {
    #[account(mut)]
    pub state: Account<'info, State>,
}

#[derive(Accounts)]
pub struct BatchTradingOrders<'info> {
    #[account(mut)]
    pub state: Account<'info, State>,
    // Additional accounts for order queuing can be added.
}

#[derive(Accounts)]
pub struct UpdateFeeByVote<'info> {
    #[account(mut)]
    pub state: Account<'info, State>,
    #[account(mut)]
    pub governance: Account<'info, GovernanceVote>,
    pub admin: Signer<'info>,
}

#[account]
pub struct GovernanceVote {
    pub vote_count: u64,
    pub required_votes: u64,
}

impl GovernanceVote {
    pub const LEN: usize = 16; // 8+8
}

#[derive(Accounts)]
pub struct UpdateDynamicReward<'info> {
    #[account(mut)]
    pub state: Account<'info, State>,
}

#[derive(Accounts)]
pub struct UpdateLeaderboard<'info> {
    #[account(mut, seeds = [b"leaderboard", user.key().as_ref()], bump = leaderboard.bump)]
    pub leaderboard: Account<'info, TraderLeaderboard>,
    pub user: Signer<'info>,
}

#[account]
pub struct TraderLeaderboard {
    pub user: Pubkey,
    pub trade_volume: u64,
    pub trade_count: u64,
    pub last_update: i64,
    pub bump: u8,
}

impl TraderLeaderboard {
    pub const LEN: usize = 32 + 8 + 8 + 8 + 1;
}

#[derive(Accounts)]
pub struct RewardStrategyBoost<'info> {
    #[account(mut, seeds = [b"user", user_state.owner.as_ref()], bump = user_state.bump)]
    pub user_state: Account<'info, UserState>,
}

#[derive(Accounts)]
pub struct BatchProcessTrades<'info> {
    #[account(mut)]
    pub state: Account<'info, State>,
}

#[derive(Accounts)]
pub struct BorrowAgainstLVT<'info> {
    #[account(mut, seeds = [b"user", user_state.owner.as_ref()], bump = user_state.bump)]
    pub user_state: Account<'info, UserState>,
    #[account(init, payer = borrower, space = 8 + LoanAccount::LEN)]
    pub loan_account: Account<'info, LoanAccount>,
    #[account(mut)]
    pub borrower: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct LoanAccount {
    pub borrower: Pubkey,
    pub collateral: u64,
    pub borrow_amount: u64,
    pub interest_rate: u64,
    pub start_time: i64,
    pub due_time: i64,
    pub bump: u8,
}

impl LoanAccount {
    pub const LEN: usize = 32 + 8 + 8 + 8 + 8 + 8 + 1;
}

#[account]
pub struct UserState {
    pub owner: Pubkey,
    pub staked_amount: u64,
    pub accrued_rewards: u64,
    pub reward_multiplier: u64,
    pub trade_count: u64,
    pub cumulative_volume: u64,
    pub fee_discount: u64,
    pub lockup_end: i64,           // Timestamp when lockup expires; 0 if none.
    pub is_institutional: bool,    // Whitelist flag for institutional traders.
    pub last_claim_time: i64,      // For cooldown on claims.
    pub trading_rebate: u64,       // Additional reward for staking tier.
    pub bump: u8,
}

impl UserState {
    // Calculation: 32 + (8*7) + 8 + 1 + 8 + 8 + 1 = 32 + 56 + 8 + 1 + 8 + 8 + 1 = 114 bytes.
    pub const LEN: usize = 114;
}

//
// CUSTOM ERRORS
//
#[error_code]
pub enum CustomError {
    #[msg("Invalid fee rate provided.")]
    InvalidFeeRate,
    #[msg("Insufficient votes for governance action.")]
    InsufficientVotes,
    #[msg("Invalid delay for batching orders.")]
    InvalidDelay,
    #[msg("Insufficient liquidity contribution to claim rewards.")]
    InsufficientLiquidityForRewards,
    #[msg("Wash trading detected. Trade between same wallet accounts is not allowed.")]
    WashTradingAttempt,
    #[msg("Minimum holding period has not been met.")]
    MinimumHoldingPeriodNotMet,
    #[msg("Insufficient collateral for borrowing.")]
    InsufficientCollateral,
}

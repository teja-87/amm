use solana_program::{
    account_info::{AccountInfo, next_account_info}, entrypoint::ProgramResult, instruction::InstructionError::ArithmeticOverflow, program::{invoke, invoke_signed}, program_error::ProgramError, program_pack::Pack, pubkey::Pubkey, system_instruction, system_program, sysvar::{Sysvar, rent::Rent},
};

use spl_token::id;
use spl_token::state::{Account as TokenAccount, Mint as MintAccount};

use borsh::{BorshDeserialize, BorshSerialize};

solana_program::entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instructions = instructionsTypes::try_from_slice(&instruction_data)
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    match instructions {
        instructionsTypes::CreateLiquiditypool => createpool(program_id, accounts),
        instructionsTypes::AddLiquidity { meme, sol } => {
            addliquidity(program_id, accounts, meme, sol)
        }
        instructionsTypes::Swap {
            amount_in,
            amount_out,
            direction,
        } => swap(program_id, accounts, amount_in, amount_out, direction),
    }
}

fn createpool(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let vault_meme = next_account_info(accounts_iter)?;
    let vault_solana = next_account_info(accounts_iter)?;
    let pool_state = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;
    let meme_mint = next_account_info(accounts_iter)?;
    let system = next_account_info(accounts_iter)?;
    let token = next_account_info(accounts_iter)?;

    // --- signer / writability checks ---
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !authority.is_writable || !pool_state.is_writable || !vault_meme.is_writable || !vault_solana.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }
    if *system.key != system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    if *token.key != id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    
    if meme_mint.owner != token.key {
        return Err(ProgramError::IllegalOwner);
    }
    let mint_data = MintAccount::unpack(&meme_mint.data.borrow())
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if !mint_data.is_initialized {
        return Err(ProgramError::UninitializedAccount);
    }

    let (pda_meme, bump1) = Pubkey::find_program_address(&[b"pda", pool_state.key.as_ref()], program_id);
    let (pda_sol, bump2) = Pubkey::find_program_address(&[b"solana", meme_mint.key.as_ref()], program_id);
    let (pda_state, bump3) = Pubkey::find_program_address(&[b"state", meme_mint.key.as_ref()], program_id);

    const FEE: u16 = 3;

    if *vault_meme.key != pda_meme {
        return Err(ProgramError::InvalidSeeds);
    }
    if *vault_solana.key != pda_sol {
        return Err(ProgramError::InvalidSeeds);
    }
    if *pool_state.key != pda_state {
        return Err(ProgramError::InvalidSeeds);
    }

   
    if pool_state.lamports() > 0 || !pool_state.data_is_empty() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    if vault_meme.lamports() > 0 || !vault_meme.data_is_empty() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    if vault_solana.lamports() > 0 || !vault_solana.data_is_empty() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    const SPACE: usize = 130;
    let rent = Rent::get()?;
    let lamports = rent.minimum_balance(SPACE);

    let transaction = system_instruction::create_account(
        authority.key,
        pool_state.key,
        lamports,
        SPACE as u64,
        program_id,
    );

    invoke_signed(
        &transaction,
        &[pool_state.clone(), authority.clone(), system.clone()],
        &[&[b"state", meme_mint.key.as_ref(), &[bump3]]],
    )?;

    let data = poolState {
        vault_meme: *vault_meme.key,
        vault_solana: *vault_solana.key,
        mint_meme: *meme_mint.key,
        authority: *authority.key,
        fee: FEE,
    };

    let mut poolstatedata = pool_state.data.borrow_mut();
    data.serialize(&mut &mut poolstatedata[..])?;
    drop(poolstatedata);

    let token_account_space = 165;
    let token_account_lamports = rent.minimum_balance(token_account_space);

    let ata_ix = system_instruction::create_account(
        authority.key,
        vault_meme.key,
        token_account_lamports,
        token_account_space as u64,
        token.key,
    );

    invoke_signed(
        &ata_ix,
        &[authority.clone(), vault_meme.clone(), system.clone()],
        &[&[b"pda", pool_state.key.as_ref(), &[bump1]]],
    )?;

    let initialize_ix = spl_token::instruction::initialize_account(
        token.key,
        vault_meme.key,
        meme_mint.key,
        pool_state.key,
    )?;
    invoke(
        &initialize_ix,
        &[
            vault_meme.clone(),
            meme_mint.clone(),
            pool_state.clone(),
            token.clone(),
        ],
    )?;

    let sol_space = 0;
    let sol_lamports = rent.minimum_balance(sol_space);

    let sol_ix = system_instruction::create_account(
        authority.key,
        vault_solana.key,
        sol_lamports,
        sol_space as u64,
        pool_state.key,
    );

    invoke_signed(
        &sol_ix,
        &[authority.clone(), vault_solana.clone(), system.clone()],
        &[&[b"solana", meme_mint.key.as_ref(), &[bump2]]],
    )?;

    Ok(())
}

fn addliquidity(program_id: &Pubkey, accounts: &[AccountInfo], amount: u64, sol: u64) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let vault_meme = next_account_info(accounts_iter)?;
    let vault_solana = next_account_info(accounts_iter)?;
    let pool_state = next_account_info(accounts_iter)?;
    let user = next_account_info(accounts_iter)?;
    let user_meme = next_account_info(accounts_iter)?;
    let meme_mint = next_account_info(accounts_iter)?;
    let system = next_account_info(accounts_iter)?;
    let token = next_account_info(accounts_iter)?;

    if !user.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !vault_meme.is_writable || !vault_solana.is_writable || !user.is_writable || !user_meme.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    if amount == 0 || sol == 0 {
        return Err(ProgramError::InvalidArgument);
    }

    if *system.key != system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    if *token.key != id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    let (pda_meme, _bump1) = Pubkey::find_program_address(&[b"pda", pool_state.key.as_ref()], program_id);
    let (pda_sol, _bump2) = Pubkey::find_program_address(&[b"solana", meme_mint.key.as_ref()], program_id);
    let (pda_state, _bump3) = Pubkey::find_program_address(&[b"state", meme_mint.key.as_ref()], program_id);

    if *vault_meme.key != pda_meme {
        return Err(ProgramError::InvalidSeeds);
    }
    if *vault_solana.key != pda_sol {
        return Err(ProgramError::InvalidSeeds);
    }
    if *pool_state.key != pda_state {
        return Err(ProgramError::InvalidSeeds);
    }

    
    if pool_state.owner != program_id {
        return Err(ProgramError::IllegalOwner);
    }
    let state_data = poolState::try_from_slice(&pool_state.data.borrow())
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if state_data.vault_meme != *vault_meme.key
        || state_data.vault_solana != *vault_solana.key
        || state_data.mint_meme != *meme_mint.key
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if user_meme.owner != token.key {
        return Err(ProgramError::IllegalOwner);
    }
    let user_meme_data = TokenAccount::unpack(&user_meme.data.borrow())
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if user_meme_data.owner != *user.key {
        return Err(ProgramError::IllegalOwner);
    }
    if user_meme_data.mint != *meme_mint.key {
        return Err(ProgramError::InvalidAccountData);
    }
    if user_meme_data.amount < amount {
        return Err(ProgramError::InsufficientFunds);
    }
    if user.lamports() < sol {
        return Err(ProgramError::InsufficientFunds);
    }

    let meme_add = spl_token::instruction::transfer(
        token.key,
        user_meme.key,
        vault_meme.key,
        user.key,
        &[],
        amount,
    )?;

    let sol_add = system_instruction::transfer(user.key, vault_solana.key, sol);

    invoke(
        &meme_add,
        &[
            user_meme.clone(),
            vault_meme.clone(),
            user.clone(),
            token.clone(),
        ],
    )?;

    invoke(&sol_add, &[user.clone(), vault_solana.clone(), system.clone()])?;

    Ok(())
}

fn swap(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount_in: u64,
    amount_out: u64,
    direction: Swapdirection,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let vault_meme = next_account_info(accounts_iter)?;
    let vault_solana = next_account_info(accounts_iter)?;
    let pool_state = next_account_info(accounts_iter)?;
    let user = next_account_info(accounts_iter)?;
    let user_meme = next_account_info(accounts_iter)?;
    let meme_mint = next_account_info(accounts_iter)?;
    let system = next_account_info(accounts_iter)?;
    let token = next_account_info(accounts_iter)?;

    if !user.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !vault_meme.is_writable || !vault_solana.is_writable || !user.is_writable || !user_meme.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

   
    if amount_in == 0 || amount_out == 0 {
        return Err(ProgramError::InvalidArgument);
    }

    if *system.key != system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    if *token.key != id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    let (pda_meme, bump1) = Pubkey::find_program_address(&[b"pda", pool_state.key.as_ref()], program_id);
    let (pda_sol, bump2) = Pubkey::find_program_address(&[b"solana", meme_mint.key.as_ref()], program_id);
    let (pda_state, bump3) = Pubkey::find_program_address(&[b"state", meme_mint.key.as_ref()], program_id);

    if *vault_meme.key != pda_meme {
        return Err(ProgramError::InvalidSeeds);
    }
    if *vault_solana.key != pda_sol {
        return Err(ProgramError::InvalidSeeds);
    }
    if *pool_state.key != pda_state {
        return Err(ProgramError::InvalidSeeds);
    }

  
    if pool_state.owner != program_id {
        return Err(ProgramError::IllegalOwner);
    }
    let state_data = poolState::try_from_slice(&pool_state.data.borrow())
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if state_data.vault_meme != *vault_meme.key
        || state_data.vault_solana != *vault_solana.key
        || state_data.mint_meme != *meme_mint.key
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if user_meme.owner != token.key {
        return Err(ProgramError::IllegalOwner);
    }
    let user_meme_data = TokenAccount::unpack(&user_meme.data.borrow())
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if user_meme_data.owner != *user.key {
        return Err(ProgramError::IllegalOwner);
    }
    if user_meme_data.mint != *meme_mint.key {
        return Err(ProgramError::InvalidAccountData);
    }

    let vault_meme_data=spl_token::state::Account::unpack(&vault_meme.data.borrow())?;  



    let x =vault_meme_data.amount;
    let y=**vault_solana.lamports.borrow();

   

 

    match direction {
        Swapdirection::Memetosol => {
            if user_meme_data.amount < amount_in {
                return Err(ProgramError::InsufficientFunds);
            }
            if vault_solana.lamports() < amount_out {
                return Err(ProgramError::InsufficientFunds);
            }           



            let present_meme =x;
            let present_solana=vault_solana.lamports();

            let k=present_meme.checked_mul(present_solana).ok_or(ProgramError::ArithmeticOverflow)?;

            let new_meme=present_meme.checked_add(amount_in).ok_or(ProgramError::ArithmeticOverflow)?;



            


            let transactionmts = spl_token::instruction::transfer(
                token.key,
                user_meme.key,
                vault_meme.key,
                user.key,
                &[],
                amount_in,
            )?;

            invoke(
                &transactionmts,
                &[
                    token.clone(),
                    user_meme.clone(),
                    user.clone(),
                    vault_meme.clone(),
                ],
            )?;


            let new_solana=k.checked_div(new_meme).ok_or(ProgramError::ArithmeticOverflow)?;

            let actual_amount_out = present_solana.checked_sub(new_solana).ok_or(ProgramError::ArithmeticOverflow)?;

            if actual_amount_out<amount_out{
                return Err(ProgramError::InsufficientFunds);
            }

            let ix_solback = system_instruction::transfer(vault_solana.key, user.key, actual_amount_out);

         
            invoke_signed(
                &ix_solback,
                &[user.clone(), vault_solana.clone(), system.clone()],
                &[&[b"solana", meme_mint.key.as_ref(), &[bump2]]],
            )?;
        }
        Swapdirection::Soltomeme => {
            if user.lamports() < amount_in {
                return Err(ProgramError::InsufficientFunds);
            }

            let vault_meme_data = TokenAccount::unpack(&vault_meme.data.borrow())
                .map_err(|_| ProgramError::InvalidAccountData)?;
            if vault_meme_data.amount < amount_out {
                return Err(ProgramError::InsufficientFunds);
            }

            let present_meme=vault_meme_data.amount;
            let y=**vault_solana.lamports.borrow();

            let k = y.checked_mul(present_meme).ok_or(ProgramError::ArithmeticOverflow)?;

            let new_sol = y.checked_add(amount_in).ok_or(ProgramError::ArithmeticOverflow)?;

            let new_meme = k.checked_div( new_sol).ok_or(ProgramError::ArithmeticOverflow)?;

            let actual_meme_out = present_meme.checked_sub( new_meme).ok_or(ProgramError::ArithmeticOverflow)?;


            let transactionstm = system_instruction::transfer(user.key, vault_solana.key, amount_in);
            invoke(
                &transactionstm,
                &[user.clone(), vault_solana.clone(), system.clone()],
            )?;

            
            if actual_meme_out< amount_out{
                return Err(ProgramError::InsufficientFunds);
            }

            

            let ix_memeback = spl_token::instruction::transfer(
                token.key,
                vault_meme.key,
                user_meme.key,
                pool_state.key,
                &[],
                actual_meme_out,
            )?;

         
            invoke_signed(
                &ix_memeback,
                &[
                    token.clone(),
                    vault_meme.clone(),
                    user_meme.clone(),
                    pool_state.clone(),
                ],
                &[&[b"state", meme_mint.key.as_ref(), &[bump3]]],
            )?;
        }
    }

    Ok(())
}

#[derive(Debug, BorshDeserialize, BorshSerialize)]
pub enum instructionsTypes {
    CreateLiquiditypool,
    Swap {
        amount_in: u64,
        amount_out: u64,
        direction: Swapdirection,
    },
    AddLiquidity {
        meme: u64,
        sol: u64,
    },
}

#[derive(Debug, BorshDeserialize, BorshSerialize)]
pub enum Swapdirection {
    Memetosol,
    Soltomeme,
}

#[derive(Debug, BorshDeserialize, BorshSerialize)]
pub struct poolState {
    vault_meme: Pubkey,
    vault_solana: Pubkey,
    mint_meme: Pubkey,
    authority: Pubkey,
    fee: u16,
}
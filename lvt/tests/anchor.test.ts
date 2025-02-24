describe("Liquidity Velocity Token", () => {
  it("initialize", async () => {
    // Generate keypairs for state, treasury, and the LVT mint.
    const stateKp = new web3.Keypair();
    const treasuryKp = new web3.Keypair();
    const mintKp = new web3.Keypair();

    // Send the initialize transaction.
    // Note: In a real environment, the treasury and mint accounts would be valid SPL token accounts.
    // For testing in Playground,  assume these keypairs suffice.
    const txHash = await pg.program.methods.initialize()
      .accounts({
        state: stateKp.publicKey,
        treasury: treasuryKp.publicKey,
        admin: pg.wallet.publicKey,
        lvtMint: mintKp.publicKey,
        systemProgram: web3.SystemProgram.programId,
        // Use the well-known SPL Token program id.
        tokenProgram: new web3.PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
      })
      .signers([stateKp])
      .rpc();
    console.log(`Use 'solana confirm -v ${txHash}' to see the logs`);

    // Confirm the transaction.
    await pg.connection.confirmTransaction(txHash);

    // Fetch the created state account.
    const stateAccount = await pg.program.account.state.fetch(stateKp.publicKey);
    console.log("On-chain state:", stateAccount);

    // Check that the initial state values are set as expected.
    assert(stateAccount.totalTrades.eq(new BN(0)));
    assert(stateAccount.totalLiquidity.eq(new BN(0)));
    assert(stateAccount.feeRate.eq(new BN(1000)));
    assert(stateAccount.treasury.equals(treasuryKp.publicKey));
  });
});

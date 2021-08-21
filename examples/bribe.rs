use anyhow::Result;
use ethers::core::{rand::thread_rng, types::transaction::eip2718::TypedTransaction};
use ethers::prelude::*;
use ethers_flashbots::*;
use std::convert::TryFrom;
use url::Url;

#[tokio::main]
async fn main() -> Result<()> {
    // Connect to the network
    let provider = Provider::<Http>::try_from("https://mainnet.eth.aragon.network")?;

    // This is your searcher identity
    let bundle_signer = LocalWallet::new(&mut thread_rng());

    // This signs transactions (given private key)
    let wallet = "add53f9a7e588d003326d1cbf9e4a43c061aadd9bc938c843a79e7b4fd2ad743"
        .parse::<LocalWallet>()?;
    
    // this is our address
    let _sender_address = wallet.address();

    // Add signer and Flashbots middleware
    let client = SignerMiddleware::new(
        FlashbotsMiddleware::new(
            provider,
            Url::parse("https://relay.flashbots.net")?,
            bundle_signer,
        ),
        wallet,
    );

    // Build a custom bundle that pays 0x0000000000000000000000000000000000000000
    let mut tx = {
        let mut inner: TypedTransaction = TransactionRequest::pay(Address::zero(), 100).into();
        client.fill_transaction(&mut inner, None).await?;
        inner
    };
    
    // some tx that is sending msg.value to block.coinbase (miner bribe)
    let mut bribe = {
        let mut inner: TypedTransaction = TransactionRequest::new().into();
        client.fill_transaction(&mut inner, None).await?;
        inner
    };

    let block_number = client.get_block_number().await?;
    let base_fee = client.get_block(block_number).await?.unwrap().base_fee_per_gas.unwrap();

    // next block can at max. have additional 12.5% gas of current block
    tx.set_gas_price(base_fee * 1125 / 1000 + 1);
    bribe.set_gas_price(base_fee * 1125 / 1000 + 1);

    // deploy a contract that does selfdestruct(block.coinbase) with payabe constructor
    bribe.set_data(Bytes::from(vec![0x60u8, 0x80u8, 0x60u8, 0x40u8, 0x52u8, 0x41u8, 0xFFu8, 0xFEu8]));
    bribe.set_nonce(tx.nonce().unwrap() + 1);

    let signed_tx = client.signer().sign_transaction(&tx).await?;
    let signed_bribe = client.signer().sign_transaction(&bribe).await?;
    let mut bundle = BundleRequest::new()
        .push_transaction(tx.rlp_signed(client.signer().chain_id(), &signed_tx))
        .push_transaction(bribe.rlp_signed(client.signer().chain_id(), &signed_bribe));
    
    bundle = bundle.set_simulation_block(block_number);
    bundle = bundle.set_block(block_number + 1);

    // Simulate it
    let simulated_bundle = client.inner().simulate_bundle(&bundle).await?;
    println!("Simulated bundle: {:?}", simulated_bundle);

    // Send it
    let pending_bundle = client.inner().send_bundle(&bundle).await?;

    // You can also optionally wait to see if the bundle was included
    match pending_bundle.await {
        Ok(bundle_hash) => println!(
            "Bundle with hash {:?} was included in target block",
            bundle_hash
        ),
        Err(PendingBundleError::BundleNotIncluded) => {
            println!("Bundle was not included in target block.")
        }
        Err(e) => println!("An error occured: {}", e),
    }

    Ok(())
}
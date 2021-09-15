use crate::*;
use near_sdk::json_types::{ValidAccountId};
use near_sdk::{ext_contract, log, Gas, PromiseResult};

const GAS_FOR_NFT_APPROVE: Gas = 10_000_000_000_000;
const GAS_FOR_RESOLVE_TRANSFER: Gas = 10_000_000_000_000;
const GAS_FOR_NFT_TRANSFER_CALL: Gas = 25_000_000_000_000 + GAS_FOR_RESOLVE_TRANSFER;
const NO_DEPOSIT: Balance = 0;

#[ext_contract(ext_non_fungible_token_receiver)]
trait NonFungibleTokenReceiver {
    /// Returns `true` if the token should be returned back to the sender.
    fn nft_on_transfer(
        &mut self,
        sender_id: AccountId,
        previous_owner_id: AccountId,
        token_id: TokenId,
        msg: String,
    ) -> Promise;
}

#[ext_contract(ext_non_fungible_approval_receiver)]
trait NonFungibleTokenApprovalsReceiver {
    fn nft_on_approve(
        &mut self,
        token_id: TokenId,
        owner_id: AccountId,
        approval_id: u64,
        msg: String,
    );
}

// TODO: create nft_on_revoke

#[ext_contract(ext_self)]
trait NonFungibleTokenResolver {
    fn nft_resolve_transfer(
        &mut self,
        owner_id: AccountId,
        receiver_id: AccountId,
        token_id: TokenId,
        approved_account_ids: HashMap<AccountId, u64>,
    ) -> bool;
}

#[near_bindgen]
impl Contract {
    #[payable]
    pub fn nft_transfer(
        &mut self,
        receiver_id: ValidAccountId,
        token_id: TokenId,
        approval_id: u64,
        memo: Option<String>,
    ) {
        assert_one_yocto();
        let sender_id = env::predecessor_account_id();
        let previous_token = self.internal_transfer(
            &sender_id,
            receiver_id.as_ref(),
            &token_id,
            Some(approval_id),
            memo,
        );
        refund_approved_account_ids(
            previous_token.owner_id.clone(),
            &previous_token.approved_account_ids,
        );
    }

    pub fn nft_payout(&self, token_id: String, balance: U128, max_len_payout: u32) -> Payout {
		let token = self.tokens_by_id.get(&token_id).expect("No token");

        // compute payouts based on balance option
        // adds in contract_royalty and computes previous owner royalty from remainder
        let owner_id = token.owner_id;
        let mut total_perpetual = 0;
        let balance_u128 = u128::from(balance);
		let mut payout: Payout = HashMap::new();
		let royalty = token.royalty;

		assert!(royalty.len() as u32 <= max_len_payout, "Market cannot payout to that many receivers");

		for (k, v) in royalty.iter() {
			let key = k.clone();
			if key != owner_id {
				payout.insert(key, royalty_to_payout(*v, balance_u128));
				total_perpetual += *v;
			}
		}
		
		// payout to contract owner - may be previous token owner, they get remainder of balance
		if self.contract_royalty > 0 && self.owner_id != owner_id {
			payout.insert(self.owner_id.clone(), royalty_to_payout(self.contract_royalty, balance_u128));
			total_perpetual += self.contract_royalty;
		}
		assert!(total_perpetual <= MINTER_ROYALTY_CAP + CONTRACT_ROYALTY_CAP, "Royalties should not be more than caps");
		// payout to previous owner
		payout.insert(owner_id, royalty_to_payout(10000 - total_perpetual, balance_u128));

		payout
	}

    #[payable]
    pub fn nft_transfer_payout(
        &mut self,
        receiver_id: ValidAccountId,
        token_id: TokenId,
        approval_id: u64,
        memo: String,
        balance: U128,
        max_len_payout: u32,
    ) -> Payout {
        assert_one_yocto();
        let sender_id = env::predecessor_account_id();
        let previous_token = self.internal_transfer(
            &sender_id,
            receiver_id.as_ref(),
            &token_id,
            Some(approval_id),
            Some(memo),
        );
        refund_approved_account_ids(
            previous_token.owner_id.clone(),
            &previous_token.approved_account_ids,
        );

        // compute payouts based on balance option
        // adds in contract_royalty and computes previous owner royalty from remainder
        let owner_id = previous_token.owner_id;
        let mut total_perpetual = 0;
        let balance_u128 = u128::from(balance);
		let mut payout: Payout = HashMap::new();
		let royalty = self.tokens_by_id.get(&token_id).expect("No token").royalty;

		assert!(royalty.len() as u32 <= max_len_payout, "Market cannot payout to that many receivers");

		for (k, v) in royalty.iter() {
			let key = k.clone();
			if key != owner_id {
				payout.insert(key, royalty_to_payout(*v, balance_u128));
				total_perpetual += *v;
			}
		}
		
		// payout to contract owner - may be previous token owner, they get remainder of balance
		if self.contract_royalty > 0 && self.owner_id != owner_id {
			payout.insert(self.owner_id.clone(), royalty_to_payout(self.contract_royalty, balance_u128));
			total_perpetual += self.contract_royalty;
		}
		assert!(total_perpetual <= MINTER_ROYALTY_CAP + CONTRACT_ROYALTY_CAP, "Royalties should not be more than caps");
		// payout to previous owner
		payout.insert(owner_id, royalty_to_payout(10000 - total_perpetual, balance_u128));

		payout
    }

    #[payable]
    pub fn nft_transfer_call(
        &mut self,
        receiver_id: ValidAccountId,
        token_id: TokenId,
        approval_id: u64,
        memo: Option<String>,
        msg: String,
    ) -> Promise {
        assert_one_yocto();
        let sender_id = env::predecessor_account_id();
        let previous_token = self.internal_transfer(
            &sender_id,
            receiver_id.as_ref(),
            &token_id,
            Some(approval_id),
            memo,
        );
        // Initiating receiver's call and the callback
        ext_non_fungible_token_receiver::nft_on_transfer(
            sender_id,
            previous_token.owner_id.clone(),
            token_id.clone(),
            msg,
            receiver_id.as_ref(),
            NO_DEPOSIT,
            env::prepaid_gas() - GAS_FOR_NFT_TRANSFER_CALL,
        )
        .then(ext_self::nft_resolve_transfer(
            previous_token.owner_id,
            receiver_id.into(),
            token_id,
            previous_token.approved_account_ids,
            &env::current_account_id(),
            NO_DEPOSIT,
            GAS_FOR_RESOLVE_TRANSFER,
        ))
    }

    #[payable]
    pub fn nft_approve(&mut self, token_id: TokenId, account_id: ValidAccountId, msg: Option<String>) {
        assert_at_least_one_yocto();
        let account_id: AccountId = account_id.into();

        let mut token = self.tokens_by_id.get(&token_id).expect("No token");

        assert_eq!(
            &env::predecessor_account_id(),
            &token.owner_id,
            "Predecessor must be the token owner."
        );

        let approval_id: u64 = token.next_approval_id;
        let is_new_approval = token
            .approved_account_ids
            .insert(account_id.clone(), approval_id)
            .is_none();

        let storage_used = if is_new_approval {
            bytes_for_approved_account_id(&account_id)
        } else {
            0
        };

        token.next_approval_id += 1;
        self.tokens_by_id.insert(&token_id, &token);

        refund_deposit(storage_used);

        if let Some(msg) = msg {
            
            // CUSTOM - add token_type to msg
            let mut final_msg = msg;
            let token_type = token.token_type;
            if let Some(token_type) = token_type {
                final_msg.insert_str(final_msg.len() - 1, &format!(",\"token_type\":\"{}\"", token_type));
            }

            ext_non_fungible_approval_receiver::nft_on_approve(
                token_id,
                token.owner_id,
                approval_id,
                final_msg,
                &account_id,
                NO_DEPOSIT,
                env::prepaid_gas() - GAS_FOR_NFT_APPROVE,
            )
            .as_return(); // Returning this promise
        }
    }

	pub fn nft_is_approved(
        &self,
        token_id: TokenId,
        approved_account_id: AccountId,
        approval_id: Option<u64>,
    ) -> bool {
        let token = self.tokens_by_id.get(&token_id).expect("No token");
		let approval = token.approved_account_ids.get(&approved_account_id);
		if let Some(approval) = approval {
			if let Some(approval_id) = approval_id {
				approval_id == *approval
			} else {
				false
			}
		} else {
			false
		}
    }

    #[payable]
    pub fn nft_revoke(&mut self, token_id: TokenId, account_id: ValidAccountId) {
        assert_one_yocto();
        let mut token = self.tokens_by_id.get(&token_id).expect("No token");
        let predecessor_account_id = env::predecessor_account_id();
        assert_eq!(&predecessor_account_id, &token.owner_id);
        if token
            .approved_account_ids
            .remove(account_id.as_ref())
            .is_some()
        {
            refund_approved_account_ids_iter(predecessor_account_id, [account_id.into()].iter());
            self.tokens_by_id.insert(&token_id, &token);
        }
    }

    #[payable]
    pub fn nft_revoke_all(&mut self, token_id: TokenId) {
        assert_one_yocto();
        let mut token = self.tokens_by_id.get(&token_id).expect("No token");
        let predecessor_account_id = env::predecessor_account_id();
        assert_eq!(&predecessor_account_id, &token.owner_id);
        if !token.approved_account_ids.is_empty() {
            refund_approved_account_ids(predecessor_account_id, &token.approved_account_ids);
            token.approved_account_ids.clear();
            self.tokens_by_id.insert(&token_id, &token);
        }
    }

    pub fn nft_total_supply(&self) -> U128 {
        U128(self.token_metadata_by_id.len() as u128)
    }

    pub fn nft_token(&self, token_id: TokenId) -> Option<JsonToken> {
        if let Some(token) = self.tokens_by_id.get(&token_id) {
            let metadata = self.token_metadata_by_id.get(&token_id).unwrap();
            Some(JsonToken {
                token_id,
                owner_id: token.owner_id,
                metadata,
                royalty: token.royalty,
                approved_account_ids: token.approved_account_ids,
                token_type: token.token_type,
            })
        } else {
            None
        }
    }

    #[private]
    pub fn nft_resolve_transfer(
        &mut self,
        owner_id: AccountId,
        receiver_id: AccountId,
        token_id: TokenId,
        approved_account_ids: HashMap<AccountId, u64>,
    ) -> bool {
        // Whether receiver wants to return token back to the sender, based on `nft_on_transfer`
        // call result.
        if let PromiseResult::Successful(value) = env::promise_result(0) {
            if let Ok(return_token) = near_sdk::serde_json::from_slice::<bool>(&value) {
                if !return_token {
                    // Token was successfully received.
                    refund_approved_account_ids(owner_id, &approved_account_ids);
                    return true;
                }
            }
        }

        let mut token = if let Some(token) = self.tokens_by_id.get(&token_id) {
            if token.owner_id != receiver_id {
                // The token is not owner by the receiver anymore. Can't return it.
                refund_approved_account_ids(owner_id, &approved_account_ids);
                return true;
            }
            token
        } else {
            // The token was burned and doesn't exist anymore.
            refund_approved_account_ids(owner_id, &approved_account_ids);
            return true;
        };

        log!("Return {} from @{} to @{}", token_id, receiver_id, owner_id);

        self.internal_remove_token_from_owner(&receiver_id, &token_id);
        self.internal_add_token_to_owner(&owner_id, &token_id);
        token.owner_id = owner_id;
        refund_approved_account_ids(receiver_id, &token.approved_account_ids);
        token.approved_account_ids = approved_account_ids;
        self.tokens_by_id.insert(&token_id, &token);

        false
    }
}

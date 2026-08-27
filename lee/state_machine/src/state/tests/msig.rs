use super::*;

use authenticated_transfer_core::Instruction as AuthTransferInstruction;

/// The REAL `authenticated_transfer` guest, rebridged into the crate-under-test's `Program`
/// type. (The `programs` dev-dependency links against the non-test build of `lee`, so its
/// `Program` is a distinct type; `ProgramId` and the ELF bytes live in `lee_core` and transfer.)
fn real_authenticated_transfer() -> Program {
    let p = programs::authenticated_transfer();
    Program::new_unchecked(p.id(), std::borrow::Cow::Owned(p.elf().to_vec()))
}


    // ─────────────────────────────────────────────────────────────────────────────────────────
    // LP-0002 (rc5 port) multisig in-process tests. Ported from v0.1.2 nssa/src/state.rs, adapting
    // to the rc5 API (LeeError, InputAccountIdentity rider model). The anonymous-approval rider is
    // now the member's LIVE shielded voting account keyed by the voting `secret` (== nsk) under
    // VOTE_IDENTIFIER 0 (review item #6), not a fresh/default rider.
    // ─────────────────────────────────────────────────────────────────────────────────────────

    #[test]
    fn msig_create_proposal_public_tx_claims_and_freezes() {
        let program = crate::test_methods::msig();
        let proposal_key = PrivateKey::try_new([7; 32]).unwrap();
        let proposal_id_acc = AccountId::from(&PublicKey::new_from_private_key(&proposal_key));
        let mut state = V03State::new();
        state.insert_program(crate::test_methods::msig());

        let member_root = [0xABu8; 32];
        let proposal_id = [0x11u8; 32];
        let instruction = msig_core::MsigInstruction::CreateProposal { member_root, proposal_id };

        let message = public_transaction::Message::try_new(
            program.id(),
            vec![proposal_id_acc],
            vec![Nonce(0)],
            instruction,
        )
        .unwrap();
        let witness_set = public_transaction::WitnessSet::for_message(&message, &[&proposal_key]);
        let tx = PublicTransaction::new(message, witness_set);
        state.transition_from_public_transaction(&tx, 1, 0).unwrap();

        let post = state.get_account_by_id(proposal_id_acc);
        assert_eq!(post.program_owner, program.id());
        let d = post.data.clone().into_inner();
        assert_eq!(&d[..32], &member_root);
        assert_eq!(&d[32..64], &proposal_id);
        assert_eq!(u32::from_le_bytes(d[64..68].try_into().unwrap()), 0);
    }

    // BLOCKED on rc5: the frozen msig guest's `execute()` chains a RAW u128 to auth_transfer
    // (`to_vec(&amount)`), but rc5's `authenticated_transfer_core::Instruction` is now an enum
    // {Transfer{amount}, Initialize}. The chained call fails to deserialize (variant-index error).
    // Re-enabling requires rebuilding the guest ELF to send `Instruction::Transfer{amount}` — out of
    // scope (guest is frozen at ImageID 7fd8..). The threshold gate + state machine itself are fine;
    // only the auth_transfer chained ABI is stale.
    // (re-enabled in the v0.2.4 port: rebuilt guest chains the typed Instruction enum)
    #[test]
    fn msig_execute_releases_at_threshold() {
        let msig = crate::test_methods::msig();
        let transfer = real_authenticated_transfer();

        let threshold: u32 = 2;
        let count: u32 = 3;
        let seed = [0u8; 32];

        let treasury_id = AccountId::for_public_pda(&msig.id(), &PdaSeed::new(seed));
        let initial_balance: u128 = 1000;
        let treasury_account = Account {
            program_owner: transfer.id(),
            balance: initial_balance,
            ..Account::default()
        };

        let recipient_id = AccountId::for_public_pda(&msig.id(), &PdaSeed::new([1u8; 32]));
        let recipient_account = Account {
            program_owner: transfer.id(),
            balance: 0,
            ..Account::default()
        };

        let proposal_id = AccountId::for_public_pda(&msig.id(), &PdaSeed::new([2u8; 32]));
        let member_root = [0xABu8; 32];
        let proposal_bytes = [0x11u8; 32];
        let mut data = Vec::new();
        data.extend_from_slice(&member_root);
        data.extend_from_slice(&proposal_bytes);
        data.extend_from_slice(&count.to_le_bytes());
        let proposal_account = Account {
            program_owner: msig.id(),
            data: data.try_into().unwrap(),
            ..Account::default()
        };

        let mut state = V03State::new().with_test_programs();
        state.insert_program(crate::test_methods::msig());
        state.insert_program(real_authenticated_transfer());
        state.force_insert_account(proposal_id, proposal_account);
        state.force_insert_account(treasury_id, treasury_account);
        state.force_insert_account(recipient_id, recipient_account);

        let instruction = msig_core::MsigInstruction::Execute { threshold, seed };
        let message = public_transaction::Message::try_new(
            msig.id(),
            vec![proposal_id, treasury_id, recipient_id],
            vec![],
            instruction,
        )
        .unwrap();
        let witness_set = public_transaction::WitnessSet::for_message(&message, &[]);
        let tx = PublicTransaction::new(message, witness_set);

        let result = state.transition_from_public_transaction(&tx, 1, 0);
        assert!(result.is_ok(), "execute should succeed: {result:?}");

        assert_eq!(state.get_account_by_id(treasury_id).balance, 0);
        assert_eq!(state.get_account_by_id(recipient_id).balance, initial_balance);
    }

    /// Proves the demo fixture COMPOSES end-to-end against in-process state, on ONE unified
    /// ProposalState account id with the depth-5 member_root. create + execute are PUBLIC txs.
    ///
    /// BLOCKED on rc5 for the same reason as `msig_execute_releases_at_threshold`: the final
    /// `Execute` chains a raw u128 to auth_transfer, incompatible with rc5's enum Instruction. The
    /// create_proposal + member_root composition (the LP-0002 core) is covered green by
    /// `msig_create_proposal_public_tx_claims_and_freezes` + the circuit approve tests.
    // (re-enabled in the v0.2.4 port: rebuilt guest chains the typed Instruction enum)
    #[test]
    fn msig_full_flow_composes() {
        use msig_core::{member_leaf, merkle_path, merkle_root, root_from_path};

        let member_secrets: [[u8; 32]; 3] = [[0xA7u8; 32], [0x42u8; 32], [0x5Cu8; 32]];
        let approver_index: usize = 0;
        let proposal_key_bytes = [7u8; 32];
        let proposal_id_bytes = [0x11u8; 32];
        let threshold: u32 = 1;
        let treasury_seed = [0u8; 32];
        let recipient_seed = [1u8; 32];

        let msig = crate::test_methods::msig();
        let transfer = real_authenticated_transfer();

        let leaves: Vec<[u8; 32]> = member_secrets.iter().map(member_leaf).collect();
        let member_root = merkle_root(&leaves);
        let approver_leaf = member_leaf(&member_secrets[approver_index]);
        let approver_path = merkle_path(&leaves, approver_index);
        assert_eq!(
            root_from_path(approver_leaf, &approver_path),
            member_root,
            "approver depth-5 path must reproduce the enrolled member_root"
        );

        let proposal_key = PrivateKey::try_new(proposal_key_bytes).unwrap();
        let proposal_id = AccountId::from(&PublicKey::new_from_private_key(&proposal_key));

        let mut state = V03State::new().with_test_programs();
        state.insert_program(crate::test_methods::msig());
        state.insert_program(real_authenticated_transfer());

        let create_ix = msig_core::MsigInstruction::CreateProposal {
            member_root,
            proposal_id: proposal_id_bytes,
        };
        let create_msg = public_transaction::Message::try_new(
            msig.id(),
            vec![proposal_id],
            vec![Nonce(0)],
            create_ix,
        )
        .unwrap();
        let create_ws = public_transaction::WitnessSet::for_message(&create_msg, &[&proposal_key]);
        let create_tx = PublicTransaction::new(create_msg, create_ws);
        state.transition_from_public_transaction(&create_tx, 1, 0).unwrap();

        let frozen = state.get_account_by_id(proposal_id);
        assert_eq!(frozen.program_owner, msig.id());
        let fd = frozen.data.clone().into_inner();
        assert_eq!(&fd[..32], &member_root, "frozen root must equal depth-5 member_root");
        assert_eq!(&fd[32..64], &proposal_id_bytes);
        assert_eq!(u32::from_le_bytes(fd[64..68].try_into().unwrap()), 0);

        // Simulate the anonymous approval reaching THRESHOLD on the SAME account.
        let mut approved_data = Vec::new();
        approved_data.extend_from_slice(&member_root);
        approved_data.extend_from_slice(&proposal_id_bytes);
        approved_data.extend_from_slice(&threshold.to_le_bytes());
        let approved_proposal = Account {
            program_owner: msig.id(),
            data: approved_data.try_into().unwrap(),
            ..Account::default()
        };
        state.force_insert_account(proposal_id, approved_proposal);

        let treasury_id = AccountId::for_public_pda(&msig.id(), &PdaSeed::new(treasury_seed));
        let initial_balance: u128 = 1000;
        state.force_insert_account(
            treasury_id,
            Account { program_owner: transfer.id(), balance: initial_balance, ..Account::default() },
        );
        let recipient_id = AccountId::for_public_pda(&msig.id(), &PdaSeed::new(recipient_seed));
        state.force_insert_account(
            recipient_id,
            Account { program_owner: transfer.id(), balance: 0, ..Account::default() },
        );

        let exec_ix = msig_core::MsigInstruction::Execute { threshold, seed: treasury_seed };
        let exec_msg = public_transaction::Message::try_new(
            msig.id(),
            vec![proposal_id, treasury_id, recipient_id],
            vec![],
            exec_ix,
        )
        .unwrap();
        let exec_ws = public_transaction::WitnessSet::for_message(&exec_msg, &[]);
        let exec_tx = PublicTransaction::new(exec_msg, exec_ws);
        let result = state.transition_from_public_transaction(&exec_tx, 1, 0);
        assert!(result.is_ok(), "execute should succeed: {result:?}");

        assert_eq!(state.get_account_by_id(treasury_id).balance, 0);
        assert_eq!(state.get_account_by_id(recipient_id).balance, initial_balance);
    }

    /// Live-apply de-risk for run_approve: the anonymous approval privacy tx must pass the FULL
    /// `transition_from_privacy_preserving_transaction` path (which reconstructs the proposal's
    /// `is_authorized` as false for the program-owned account), not just `proof.is_valid_for`.
    /// rc5 port: the rider is the member's LIVE shielded voting account keyed by `secret` (== nsk)
    /// under VOTE_IDENTIFIER 0 (review item #6), seeded on-chain so its membership proof exists.
    #[test]
    fn msig_approve_live_apply_is_authorized_false() {
        use msig_core::{member_leaf, merkle_path, merkle_root};

        let msig = crate::test_methods::msig();

        let member_secrets: [[u8; 32]; 3] = [[0xA7u8; 32], [0x42u8; 32], [0x5Cu8; 32]];
        let leaves: Vec<[u8; 32]> = member_secrets.iter().map(member_leaf).collect();
        let member_root = merkle_root(&leaves);
        let path = merkle_path(&leaves, 0);
        let approver_secret = member_secrets[0];
        let proposal_id_bytes = [0x11u8; 32];

        let proposal_key = PrivateKey::try_new([7u8; 32]).unwrap();
        let proposal_id_acc = AccountId::from(&PublicKey::new_from_private_key(&proposal_key));

        // The member's LIVE voting account: nsk == approver_secret, identifier 0, msig-owned.
        let rider_keys = TestPrivateKeys { nsk: approver_secret, d: [0x31; 32], z: [0x32; 32] };
        let rider_id = AccountId::for_regular_private_account(&rider_keys.npk(), &rider_keys.vpk(), 0);
        let rider_account = Account {
            program_owner: msig.id(),
            balance: 1,
            ..Account::default()
        };

        let mut state = V03State::new()
            .with_test_programs()
            .with_private_account(&rider_keys, &rider_account);
        state.insert_program(crate::test_methods::msig());

        // (1) Real CreateProposal public tx → proposal owned by msig, count 0, nonce bumped.
        let create_ix = msig_core::MsigInstruction::CreateProposal {
            member_root,
            proposal_id: proposal_id_bytes,
        };
        let create_msg = public_transaction::Message::try_new(
            msig.id(),
            vec![proposal_id_acc],
            vec![Nonce(0)],
            create_ix,
        )
        .unwrap();
        let create_ws = public_transaction::WitnessSet::for_message(&create_msg, &[&proposal_key]);
        let create_tx = PublicTransaction::new(create_msg, create_ws);
        state.transition_from_public_transaction(&create_tx, 1, 0).unwrap();

        let live = state.get_account_by_id(proposal_id_acc);
        assert_eq!(live.nonce, Nonce(1), "post-create nonce must be 1");

        // (2) Build approve like the hardened runner: proposal from live, is_authorized = FALSE.
        let proposal = AccountWithMetadata::new(live.clone(), false, proposal_id_acc);
        let rider_commitment = Commitment::new(&rider_id, &rider_account);
        let rider = AccountWithMetadata::new(rider_account.clone(), true, rider_id);

        let instruction = Program::serialize_instruction(msig_core::MsigInstruction::Approve {
            secret: approver_secret,
            merkle_path: path,
            proposal_id: proposal_id_bytes,
            vpk: rider_keys.vpk(),
        })
        .unwrap();

        let (output, proof) = crate::privacy_preserving_transaction::circuit::execute_and_prove(
            vec![proposal, rider],
            instruction,
            vec![
                InputAccountIdentity::Public,
                InputAccountIdentity::PrivateAuthorizedUpdate {
                    vpk: rider_keys.vpk(),
                    random_seed: [0; 32],
                    view_tag: 0,
                    nsk: rider_keys.nsk,
                    membership_proof: state
                        .get_proof_for_commitment(&rider_commitment)
                        .expect("rider commitment must be in state"),
                    identifier: 0,
                },
            ],
            &msig.clone().into(),
        )
        .unwrap();
        assert!(proof.is_valid_for(&output), "guest must accept is_authorized=false");

        let message = Message::from_circuit_output(vec![], output);
        let witness_set = WitnessSet::for_message(&message, proof, &[]);
        let tx = PrivacyPreservingTransaction::new(message, witness_set);

        // (3) FULL apply path — this is what the live sequencer runs.
        let result = state.transition_from_privacy_preserving_transaction(&tx, 2, 0);
        assert!(
            result.is_ok(),
            "approve must pass live apply with is_authorized=false: {result:?}"
        );

        let post = state.get_account_by_id(proposal_id_acc);
        let pd = post.data.clone().into_inner();
        assert_eq!(u32::from_le_bytes(pd[64..68].try_into().unwrap()), 1, "count must be 1");
    }

    /// Captures the exact apply-time rejection for the enroll PUBLIC tx as built by run_enroll
    /// (registry referenced with no signer, no nonce, no PDA seed). The guest claims the registry
    /// `Authorized`, but apply reconstructs the PDA's authorization as false → rejection.
    #[test]
    fn msig_enroll_public_tx_apply_rejection() {
        let msig = crate::test_methods::msig();
        let registry_id = AccountId::for_public_pda(&msig.id(), &PdaSeed::new([0xCCu8; 32]));

        let mut state = V03State::new().with_test_programs();
        state.insert_program(crate::test_methods::msig());

        let leaf = msig_core::member_leaf(&[0xA7u8; 32]);
        let instruction = msig_core::MsigInstruction::Enroll { leaf };
        let message = public_transaction::Message::try_new(
            msig.id(),
            vec![registry_id],
            vec![],
            instruction,
        )
        .unwrap();
        let witness_set = public_transaction::WitnessSet::for_message(&message, &[]);
        let tx = PublicTransaction::new(message, witness_set);

        let result = state.transition_from_public_transaction(&tx, 1, 0);
        println!("ENROLL_APPLY_RESULT: {result:?}");
        assert!(result.is_err(), "expected enroll to reject at apply (PDA not authorized)");
    }

    /// Why a plain authenticated_transfer fund to an uninitialized treasury PDA is dropped. Three
    /// recipient arms isolate the rule: (a) fresh recipient that does NOT co-sign → fail,
    /// (b) fresh recipient that DOES co-sign → succeed, (c) the msig treasury PDA → fail with the
    /// same ClaimedUnauthorizedAccount as (a). No PDA-specific rule: the credit of a fresh recipient
    /// emits Claim::Authorized, accepted only when the recipient is a signer; a PDA can never sign.
    #[test]
    fn msig_fund_treasury_pda_rejected() {
        let transfer = real_authenticated_transfer();
        let msig = crate::test_methods::msig();
        let amount: u128 = 50;

        let sender_key = PrivateKey::try_new([99u8; 32]).unwrap();
        let sender_id = AccountId::from(&PublicKey::new_from_private_key(&sender_key));

        let run = |recipient_id: AccountId, signers: &[&PrivateKey], nonces: Vec<Nonce>| {
            let mut state =
                V03State::new()
                    .with_public_accounts(std::collections::HashMap::from([(sender_id, Account {
                program_owner: real_authenticated_transfer().id(),
                balance: 150u128,
                ..Account::default()
            })]))
                    .with_test_programs();
            state.insert_program(crate::test_methods::msig());
        state.insert_program(real_authenticated_transfer());

            let message = public_transaction::Message::try_new(
                transfer.id(),
                vec![sender_id, recipient_id],
                nonces,
                AuthTransferInstruction::Transfer { amount },
            )
            .unwrap();
            let witness_set = public_transaction::WitnessSet::for_message(&message, signers);
            let tx = PublicTransaction::new(message, witness_set);
            (state.transition_from_public_transaction(&tx, 1, 0), state)
        };

        let plain_key = PrivateKey::try_new([77u8; 32]).unwrap();
        let plain_id = AccountId::from(&PublicKey::new_from_private_key(&plain_key));
        let (res_a, _) = run(plain_id, &[&sender_key], vec![Nonce(0)]);
        println!("PROBE_A (plain recipient, no co-sign): {res_a:?}");
        assert!(res_a.is_err(), "plain non-signing recipient must fail");

        let (res_b, state_b) = run(plain_id, &[&sender_key, &plain_key], vec![Nonce(0), Nonce(0)]);
        println!("PROBE_B (plain recipient, co-signed): {res_b:?}");
        assert!(res_b.is_ok(), "co-signed plain recipient must succeed: {res_b:?}");
        assert_eq!(state_b.get_account_by_id(plain_id).balance, amount);
        assert_eq!(state_b.get_account_by_id(plain_id).program_owner, transfer.id());

        let treasury_id = AccountId::for_public_pda(&msig.id(), &PdaSeed::new([0u8; 32]));
        let (res_c, _) = run(treasury_id, &[&sender_key], vec![Nonce(0)]);
        println!("PROBE_C (treasury PDA, the dropped fund): {res_c:?}");
        assert!(res_c.is_err(), "fresh treasury PDA fund must fail");

        assert!(
            matches!(
                res_a,
                Err(LeeError::InvalidProgramBehavior(
                    InvalidProgramBehaviorError::ClaimedUnauthorizedAccount { .. }
                ))
            ),
            "arm (a) must be ClaimedUnauthorizedAccount, got {res_a:?}"
        );
        assert!(
            matches!(
                res_c,
                Err(LeeError::InvalidProgramBehavior(
                    InvalidProgramBehaviorError::ClaimedUnauthorizedAccount { .. }
                ))
            ),
            "arm (c) must be ClaimedUnauthorizedAccount, got {res_c:?}"
        );
    }

    /// The on-chain-reproducible treasury bootstrap, end to end: InitTreasury(treasury) → fund →
    /// InitTreasury(recipient) → CreateProposal → simulate approval at THRESHOLD → Execute drains.
    ///
    /// BLOCKED on rc5: both `InitTreasury` (chains `to_vec(&0_u128)`) and `Execute` (chains
    /// `to_vec(&amount)`) send a raw u128 to auth_transfer, but rc5's auth_transfer Instruction is an
    /// enum (init should send `Initialize`, drain should send `Transfer{amount}`). Frozen-guest ABI
    /// gap — out of scope to fix here.
    // (re-enabled in the v0.2.4 port: rebuilt guest chains the typed Instruction enum)
    #[test]
    fn msig_treasury_bootstrap_then_execute() {
        use msig_core::{member_leaf, merkle_root};

        let msig = crate::test_methods::msig();
        let transfer = real_authenticated_transfer();
        let transfer_id_words: [u32; 8] = transfer.id();

        let member_secrets: [[u8; 32]; 3] = [[0xA7u8; 32], [0x42u8; 32], [0x5Cu8; 32]];
        let proposal_key_bytes = [7u8; 32];
        let proposal_id_bytes = [0x11u8; 32];
        let threshold: u32 = 1;
        let treasury_seed = [0u8; 32];
        let recipient_seed = [1u8; 32];
        let fund_amount: u128 = 1000;

        let leaves: Vec<[u8; 32]> = member_secrets.iter().map(member_leaf).collect();
        let member_root = merkle_root(&leaves);

        let treasury_id = AccountId::for_public_pda(&msig.id(), &PdaSeed::new(treasury_seed));
        let recipient_id = AccountId::for_public_pda(&msig.id(), &PdaSeed::new(recipient_seed));

        let funder_key = PrivateKey::try_new([99u8; 32]).unwrap();
        let funder_id = AccountId::from(&PublicKey::new_from_private_key(&funder_key));

        let mut state = V03State::new()
            .with_public_accounts(std::collections::HashMap::from([(funder_id, Account {
                program_owner: real_authenticated_transfer().id(),
                balance: 5000u128,
                ..Account::default()
            })]))
            .with_test_programs();
        state.insert_program(crate::test_methods::msig());
        state.insert_program(real_authenticated_transfer());

        assert_eq!(state.get_account_by_id(treasury_id), Account::default());
        assert_eq!(state.get_account_by_id(recipient_id), Account::default());

        let init_treasury_ix = msig_core::MsigInstruction::InitTreasury {
            seed: treasury_seed,
            transfer_program_id: transfer_id_words,
        };
        let init_treasury_msg = public_transaction::Message::try_new(
            msig.id(),
            vec![treasury_id],
            vec![],
            init_treasury_ix,
        )
        .unwrap();
        let init_treasury_ws =
            public_transaction::WitnessSet::for_message(&init_treasury_msg, &[]);
        let init_treasury_tx = PublicTransaction::new(init_treasury_msg, init_treasury_ws);
        state
            .transition_from_public_transaction(&init_treasury_tx, 1, 0)
            .expect("InitTreasury(treasury) must succeed");

        let t = state.get_account_by_id(treasury_id);
        assert_eq!(t.program_owner, transfer.id(), "treasury now auth-transfer-owned");
        assert_eq!(t.balance, 0, "treasury initialized at balance 0");

        let fund_msg = public_transaction::Message::try_new(
            transfer.id(),
            vec![funder_id, treasury_id],
            vec![Nonce(0)],
            AuthTransferInstruction::Transfer { amount: fund_amount },
        )
        .unwrap();
        let fund_ws = public_transaction::WitnessSet::for_message(&fund_msg, &[&funder_key]);
        let fund_tx = PublicTransaction::new(fund_msg, fund_ws);
        state
            .transition_from_public_transaction(&fund_tx, 2, 0)
            .expect("funding the owned treasury must succeed");
        assert_eq!(state.get_account_by_id(treasury_id).balance, fund_amount, "treasury funded");

        let init_recip_ix = msig_core::MsigInstruction::InitTreasury {
            seed: recipient_seed,
            transfer_program_id: transfer_id_words,
        };
        let init_recip_msg = public_transaction::Message::try_new(
            msig.id(),
            vec![recipient_id],
            vec![],
            init_recip_ix,
        )
        .unwrap();
        let init_recip_ws = public_transaction::WitnessSet::for_message(&init_recip_msg, &[]);
        let init_recip_tx = PublicTransaction::new(init_recip_msg, init_recip_ws);
        state
            .transition_from_public_transaction(&init_recip_tx, 3, 0)
            .expect("InitTreasury(recipient) must succeed");
        assert_eq!(
            state.get_account_by_id(recipient_id).program_owner,
            transfer.id(),
            "recipient now auth-transfer-owned"
        );

        let proposal_key = PrivateKey::try_new(proposal_key_bytes).unwrap();
        let proposal_id = AccountId::from(&PublicKey::new_from_private_key(&proposal_key));
        let create_ix = msig_core::MsigInstruction::CreateProposal {
            member_root,
            proposal_id: proposal_id_bytes,
        };
        let create_msg = public_transaction::Message::try_new(
            msig.id(),
            vec![proposal_id],
            vec![Nonce(0)],
            create_ix,
        )
        .unwrap();
        let create_ws = public_transaction::WitnessSet::for_message(&create_msg, &[&proposal_key]);
        let create_tx = PublicTransaction::new(create_msg, create_ws);
        state
            .transition_from_public_transaction(&create_tx, 4, 0)
            .expect("CreateProposal must succeed");

        let mut approved_data = Vec::new();
        approved_data.extend_from_slice(&member_root);
        approved_data.extend_from_slice(&proposal_id_bytes);
        approved_data.extend_from_slice(&threshold.to_le_bytes());
        state.force_insert_account(
            proposal_id,
            Account {
                program_owner: msig.id(),
                data: approved_data.try_into().unwrap(),
                ..Account::default()
            },
        );

        let exec_ix = msig_core::MsigInstruction::Execute { threshold, seed: treasury_seed };
        let exec_msg = public_transaction::Message::try_new(
            msig.id(),
            vec![proposal_id, treasury_id, recipient_id],
            vec![],
            exec_ix,
        )
        .unwrap();
        let exec_ws = public_transaction::WitnessSet::for_message(&exec_msg, &[]);
        let exec_tx = PublicTransaction::new(exec_msg, exec_ws);
        state
            .transition_from_public_transaction(&exec_tx, 5, 0)
            .expect("Execute must drain the bootstrapped treasury");

        assert_eq!(state.get_account_by_id(treasury_id).balance, 0, "treasury drained");
        assert_eq!(
            state.get_account_by_id(recipient_id).balance,
            fund_amount,
            "recipient received the full treasury"
        );
    }

    /// The enroll BUG-1 fix, client-side. The registry is a SIGNER-OWNED account (a dedicated
    /// registry keypair), not a PDA. Each enroll tx is signed by that key, so the guest's
    /// `Claim::Authorized` of the registry passes apply. Drives 3 Enroll public txs and asserts the
    /// registry root == merkle_root(demo leaves) and leaf_count == 3.
    #[test]
    fn msig_enroll_signer_owned_appends() {
        use msig_core::{member_leaf, merkle_root};

        let msig = crate::test_methods::msig();
        let member_secrets: [[u8; 32]; 3] = [[0xA7u8; 32], [0x42u8; 32], [0x5Cu8; 32]];
        let leaves: Vec<[u8; 32]> = member_secrets.iter().map(member_leaf).collect();
        let expected_root = merkle_root(&leaves);

        let registry_key = PrivateKey::try_new([0xCCu8; 32]).unwrap();
        let registry_id = AccountId::from(&PublicKey::new_from_private_key(&registry_key));

        let mut state = V03State::new().with_test_programs();
        state.insert_program(crate::test_methods::msig());

        for (i, leaf) in leaves.iter().enumerate() {
            let nonce = Nonce(i as u128);
            let instruction = msig_core::MsigInstruction::Enroll { leaf: *leaf };
            let message = public_transaction::Message::try_new(
                msig.id(),
                vec![registry_id],
                vec![nonce],
                instruction,
            )
            .unwrap();
            let witness_set =
                public_transaction::WitnessSet::for_message(&message, &[&registry_key]);
            let tx = PublicTransaction::new(message, witness_set);
            state
                .transition_from_public_transaction(&tx, (i + 1) as u64, 0)
                .unwrap_or_else(|e| panic!("enroll {i} (signer-owned registry) must succeed: {e:?}"));
        }

        let reg = state.get_account_by_id(registry_id);
        assert_eq!(reg.program_owner, msig.id(), "registry is msig-owned after first claim");
        let d = reg.data.clone().into_inner();
        assert_eq!(&d[..32], &expected_root, "registry root == demo member_root");
        assert_eq!(u32::from_le_bytes(d[32..36].try_into().unwrap()), 3, "leaf_count == 3");
    }


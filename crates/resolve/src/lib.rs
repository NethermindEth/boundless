// Copyright (c) 2024 RISC Zero, Inc.
//
// All rights reserved.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use core::fmt::Debug;
use risc0_aggregation::SetInclusionReceipt;
use risc0_zkvm::sha::Digest;
use risc0_zkvm::{InnerAssumptionReceipt, ReceiptClaim, Unknown};
use serde::{Deserialize, Serialize};

/// Receipt attesting to the validity of an assumption.
///
/// This enum is an extension of [risc0_zkvm::AssumptionReceipt], adding a variant to support
/// [SetInclusionReceipt] so that aggregated receipts can be used to resolve assumptions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AssumptionReceipt {
    /// Base assumption receipt
    Base(InnerAssumptionReceipt),
    /// Set inclusion receipt for aggregated proofs
    SetInclusion { image_id: Digest, receipt: SetInclusionReceipt<Unknown> },
}

/// Input of the Resolve guest.
///
/// Resolve guest takes as input two claims, one that is conditional on the other. It verifies both
/// there is a proof for the conditional claim, and that there is a proof for the assumption and
/// then removes the assumption from the conditional claim, as it is no longer assumed, it is
/// proven.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolveInput {
    /// Conditional claim with at least on assumption on which we want to resolve the first.
    ///
    /// The conditional claim will be verified by composition, using the recursion circuit to
    /// verify the existence of a [SuccinctReceipt][risc0_zkvm::SuccinctReceipt] attesting to the
    /// validity of the claim. This receipt should be added to the
    /// [ExecutorEnv][risc0_zkvm::ExecutorEnv] when proving the (rv32im) resolve program.
    pub conditional: ReceiptClaim,

    /// Receipt attesting to the validity of the assumption.
    pub assumption: AssumptionReceipt,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, string::ToString, vec, vec::Vec};
    use anyhow::Context;
    use core::iter;
    use guest_resolve::{RESOLVE_GUEST_ELF, RESOLVE_GUEST_ID};
    use guest_set_builder::{SET_BUILDER_ELF, SET_BUILDER_ID};
    use guest_util::{ECHO_ELF, ECHO_ID, IDENTITY_ELF, IDENTITY_ID};
    use risc0_aggregation::{
        GuestState as AggregationState, SetInclusionReceiptVerifierParameters,
    };
    use risc0_zkvm::{
        default_prover,
        sha::{Digest, Digestible},
        Assumption, ExecutorEnv, ExecutorImpl, FakeReceipt, InnerReceipt, MaybePruned, ProveInfo,
        ProverOpts, Receipt, VerifierContext,
    };
    use test_log::test;

    /// Produce a receipt for the guest execution.
    fn prove_with_opts(env: ExecutorEnv, elf: &[u8], opts: &ProverOpts) -> anyhow::Result<Receipt> {
        tracing::debug!("starting proving");
        let ProveInfo { receipt, stats } = default_prover().prove_with_opts(env, elf, opts)?;
        tracing::debug!("finished proving: {:#?}", stats);
        Ok(receipt)
    }

    fn echo(input: &str) -> anyhow::Result<Receipt> {
        let env = ExecutorEnv::builder().write(&input.as_bytes())?.build()?;
        prove_with_opts(env, ECHO_ELF, &ProverOpts::succinct())
    }

    /// Processes a receipt and returns a conditional receipt based on the execution environment.
    fn identity_conditional(input: &Receipt) -> anyhow::Result<Receipt> {
        // Extract claim and build environment
        let claim = input.claim().context("Invalid claim")?.value().context("Pruned claim")?;
        let env = ExecutorEnv::builder().write(&claim)?.add_assumption(claim.clone()).build()?;

        let session = ExecutorImpl::from_elf(env, IDENTITY_ELF)
            .context("Failed to create executor")?
            .run()
            .context("Failed to run executor")?;

        // prove_segment does not work in dev mode, so we have to create the conditional receipt
        // TODO(victor): Make prove_segment work in dev mode
        if risc0_zkvm::is_dev_mode() {
            let journal = session.journal.unwrap();
            let assumption = Assumption { claim: claim.digest(), control_root: Digest::ZERO };
            let mut claim = ReceiptClaim::ok(IDENTITY_ID, MaybePruned::Pruned(journal.digest()));
            claim.output.as_value_mut().unwrap().as_mut().unwrap().assumptions =
                vec![assumption].into();

            return Ok(Receipt::new(InnerReceipt::Fake(FakeReceipt::new(claim)), journal.bytes));
        }

        let prover = risc0_zkvm::get_prover_server(&ProverOpts::succinct())?;

        // The default_prover() only works when all the assumptions are resolved, thus compress the
        // segments manually but do not resolve the assumptions.
        // TODO(risc0#982): Support conditional receipts in proof composition
        tracing::debug!("starting proving");
        let conditional_receipt = session
            .segments
            .iter()
            .map(|segment| {
                let resolved_segment = segment.resolve().context("Failed to resolve segment")?;
                let segment_receipt = prover
                    .prove_segment(&VerifierContext::default(), &resolved_segment)
                    .context("Failed to prove segment")?;
                prover.lift(&segment_receipt).context("Failed to lift")
            })
            .reduce(|left, right| prover.join(&left?, &right?).context("Failed to join"))
            .context("No segments")??;
        tracing::debug!("finished proving: {:#?}", session.stats());

        Ok(Receipt::new(
            InnerReceipt::Succinct(conditional_receipt),
            session.journal.unwrap().bytes,
        ))
    }

    /// Produce receipt for an aggregation of the given list of receipts.
    fn prove_set(receipts: Vec<Receipt>, opts: &ProverOpts) -> anyhow::Result<Receipt> {
        let input = AggregationState::initial(SET_BUILDER_ID).into_input(
            receipts.iter().map(|r| r.claim().unwrap().value().unwrap()).collect(),
            true,
        )?;

        let mut builder = ExecutorEnv::builder();
        let builder = builder.write(&input)?;
        let env = receipts
            .into_iter()
            .fold(builder, |builder, receipt| builder.add_assumption(receipt))
            .build()?;

        prove_with_opts(env, SET_BUILDER_ELF, opts)
    }

    #[test]
    #[ignore = "does not work in dev mode"]
    fn resolve_basic() {
        let echo_receipt = echo("echo... echo... echo...").unwrap();
        echo_receipt.inner.succinct().unwrap();
        echo_receipt.verify(ECHO_ID).unwrap();

        let conditional_receipt = identity_conditional(&echo_receipt).unwrap();
        conditional_receipt.verify(IDENTITY_ID).unwrap_err();

        let input = ResolveInput {
            conditional: conditional_receipt.claim().unwrap().value().unwrap(),
            assumption: AssumptionReceipt::Base(echo_receipt.inner.into()),
        };

        // Proving should fail if the conditional receipt is not provided.
        let env = ExecutorEnv::builder()
            .write_frame(&postcard::to_allocvec(&input).unwrap())
            .build()
            .unwrap();
        let err = prove_with_opts(env, RESOLVE_GUEST_ELF, &ProverOpts::succinct()).unwrap_err();
        assert!(err.to_string().contains("no receipt found to resolve assumption"), "{err}");

        let env = ExecutorEnv::builder()
            .write_frame(&postcard::to_allocvec(&input).unwrap())
            .add_assumption(conditional_receipt)
            .build()
            .unwrap();
        let receipt = prove_with_opts(env, RESOLVE_GUEST_ELF, &ProverOpts::succinct()).unwrap();
        receipt.verify(RESOLVE_GUEST_ID).unwrap();

        let resolved_claim_digest = Digest::try_from(receipt.journal.bytes).unwrap();
        assert_eq!(resolved_claim_digest, ReceiptClaim::ok(IDENTITY_ID, vec![]).digest());
    }

    #[test]
    fn resolve_set_inclusion() {
        let echo_receipt = echo("echo... echo... echo...").unwrap();
        echo_receipt.verify(ECHO_ID).unwrap();
        let echo_claim = echo_receipt.claim().unwrap();

        // Produce the conditional receipt. Verifying it should fail.
        let conditional_receipt = identity_conditional(&echo_receipt).unwrap();
        conditional_receipt.verify(IDENTITY_ID).unwrap_err();

        let receipts: Vec<_> = iter::once(echo_receipt)
            .chain((1..8).map(|i| echo(&format!("{i}")).unwrap()))
            .collect();
        let digests: Vec<_> = receipts.iter().map(|r| r.claim().unwrap().digest()).collect();
        let set_inclusion_receipt = prove_set(receipts, &ProverOpts::succinct()).unwrap();

        // NOTE: A root is not provided since we'll resolve the root assumption via composition. We
        // use fake receipts in this test, but this mirrors the case where the prover has a STARK
        // receipt for the root.
        let verifier_parameters =
            SetInclusionReceiptVerifierParameters { image_id: Digest::from(SET_BUILDER_ID) };
        let singleton_inclusion_receipt = SetInclusionReceipt::from_path_with_verifier_params(
            MaybePruned::Pruned(echo_claim.digest()),
            risc0_aggregation::merkle_path(&digests, 0),
            verifier_parameters.digest(),
        );

        let input = ResolveInput {
            conditional: conditional_receipt.claim().unwrap().value().unwrap(),
            assumption: AssumptionReceipt::SetInclusion {
                image_id: SET_BUILDER_ID.into(),
                receipt: singleton_inclusion_receipt,
            },
        };

        // Run with the conditional receipt, and the set receipt for the singleton as assumptions.
        let env = ExecutorEnv::builder()
            .write_frame(&postcard::to_allocvec(&input).unwrap())
            .add_assumption(conditional_receipt)
            .add_assumption(set_inclusion_receipt)
            .build()
            .unwrap();
        let receipt = prove_with_opts(env, RESOLVE_GUEST_ELF, &ProverOpts::succinct()).unwrap();
        receipt.verify(RESOLVE_GUEST_ID).unwrap();

        let resolved_claim_digest = Digest::try_from(receipt.journal.bytes).unwrap();
        assert_eq!(resolved_claim_digest, ReceiptClaim::ok(IDENTITY_ID, vec![]).digest());
    }

    #[test]
    #[ignore = "does not work in dev mode"]
    fn resolve_set_inclusion_with_groth16_root() {
        let echo_receipt = echo("echo... echo... echo...").unwrap();
        echo_receipt.verify(ECHO_ID).unwrap();
        let echo_claim = echo_receipt.claim().unwrap();

        // Produce the conditional receipt. Verifying it should fail.
        let conditional_receipt = identity_conditional(&echo_receipt).unwrap();
        conditional_receipt.verify(IDENTITY_ID).unwrap_err();

        let receipts: Vec<_> = iter::once(echo_receipt)
            .chain((1..8).map(|i| echo(&format!("{i}")).unwrap()))
            .collect();
        let digests: Vec<_> = receipts.iter().map(|r| r.claim().unwrap().digest()).collect();
        let set_inclusion_receipt = prove_set(receipts, &ProverOpts::groth16()).unwrap();

        let verifier_parameters =
            SetInclusionReceiptVerifierParameters { image_id: Digest::from(SET_BUILDER_ID) };
        let singleton_inclusion_receipt = SetInclusionReceipt::from_path_with_verifier_params(
            MaybePruned::Pruned(echo_claim.digest()),
            risc0_aggregation::merkle_path(&digests, 0),
            verifier_parameters.digest(),
        )
        .with_root(set_inclusion_receipt);

        let input = ResolveInput {
            conditional: conditional_receipt.claim().unwrap().value().unwrap(),
            assumption: AssumptionReceipt::SetInclusion {
                image_id: SET_BUILDER_ID.into(),
                receipt: singleton_inclusion_receipt,
            },
        };

        // Run with the conditional receipt as an assumptions and the set receipt containing a
        // succinct STARK receipt for direct verification of the root.
        let env = ExecutorEnv::builder()
            .write_frame(&postcard::to_allocvec(&input).unwrap())
            .add_assumption(conditional_receipt)
            .build()
            .unwrap();
        let receipt = prove_with_opts(env, RESOLVE_GUEST_ELF, &ProverOpts::succinct()).unwrap();
        receipt.verify(RESOLVE_GUEST_ID).unwrap();

        let resolved_claim_digest = Digest::try_from(receipt.journal.bytes).unwrap();
        assert_eq!(resolved_claim_digest, ReceiptClaim::ok(IDENTITY_ID, vec![]).digest());
    }
}

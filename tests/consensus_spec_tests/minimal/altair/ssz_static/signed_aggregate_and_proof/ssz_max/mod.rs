// WARNING!
// This file was generated with `gen-tests`. Do NOT edit manually.

use crate::spec_test_runners::ssz_static::SignedAggregateAndProofTestCase;
use crate::test_utils::TestCase;

#[test]
fn test_case_0() {
    let  test_case = SignedAggregateAndProofTestCase::from("consensus-spec-tests/tests/minimal/altair/ssz_static/SignedAggregateAndProof/ssz_max/case_0");

    test_case.execute();
}

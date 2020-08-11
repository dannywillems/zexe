use algebra::mnt4_753::{Fq, Fq2Parameters, Fq4Parameters};

use crate::fields::{fp::FpVar, fp2::Fp2Var, fp4::Fp4Var};

pub type FqVar = FpVar<Fq>;
pub type Fq2Var = Fp2Var<Fq2Parameters>;
pub type Fq4Var = Fp4Var<Fq4Parameters>;

#[test]
fn mnt4_753_field_gadgets_test() {
    use super::*;
    use crate::fields::tests::*;
    use algebra::mnt4_753::{Fq, Fq2, Fq4};

    field_test::<_, FqVar>().unwrap();
    frobenius_tests::<Fq, FqVar>(13).unwrap();

    field_test::<_, Fq2Var>().unwrap();
    frobenius_tests::<Fq2, Fq2Var>(13).unwrap();

    field_test::<_, Fq4Var>().unwrap();
    frobenius_tests::<Fq4, Fq4Var>(13).unwrap();
}

use algebra::{
    fields::{CubicExtField, CubicExtParameters, Field},
    One,
};
use core::{borrow::Borrow, marker::PhantomData};
use r1cs_core::{ConstraintSystemRef, Namespace, SynthesisError};

use crate::{
    fields::{FieldOpsBounds, FieldVar},
    prelude::*,
    Assignment, Vec,
};

#[derive(Derivative)]
#[derivative(Debug(bound = "BF: core::fmt::Debug"), Clone(bound = "BF: Clone"))]
#[must_use]
pub struct CubicExtVar<BF: FieldVar<P::BaseField>, P: CubicExtVarParams<BF>>
where
    for<'a> &'a BF: FieldOpsBounds<'a, P::BaseField, BF>,
{
    pub c0: BF,
    pub c1: BF,
    pub c2: BF,
    #[derivative(Debug = "ignore")]
    _params: PhantomData<P>,
}

pub trait CubicExtVarParams<BF: FieldVar<Self::BaseField>>: CubicExtParameters
where
    for<'a> &'a BF: FieldOpsBounds<'a, Self::BaseField, BF>,
{
    fn mul_base_field_vars_by_frob_coeff(c1: &mut BF, c2: &mut BF, power: usize);
}

impl<BF: FieldVar<P::BaseField>, P: CubicExtVarParams<BF>> CubicExtVar<BF, P>
where
    for<'a> &'a BF: FieldOpsBounds<'a, P::BaseField, BF>,
{
    #[inline]
    pub fn new(c0: BF, c1: BF, c2: BF) -> Self {
        let _params = PhantomData;
        Self {
            c0,
            c1,
            c2,
            _params,
        }
    }

    /// Multiply a BF by cubic nonresidue P::NONRESIDUE.
    #[inline]
    pub fn mul_base_field_by_nonresidue(fe: &BF) -> Result<BF, SynthesisError> {
        Ok(fe * P::NONRESIDUE)
    }

    /// Multiply a CubicExtVar by an element of `P::BaseField`.
    #[inline]
    pub fn mul_by_base_field_constant(&self, fe: P::BaseField) -> Self {
        let c0 = &self.c0 * fe;
        let c1 = &self.c1 * fe;
        let c2 = &self.c2 * fe;
        Self::new(c0, c1, c2)
    }

    #[inline]
    pub fn mul_assign_by_base_field_constant(&mut self, fe: P::BaseField) {
        *self = (&*self).mul_by_base_field_constant(fe);
    }
}

impl<BF, P> R1CSVar<BF::ConstraintF> for CubicExtVar<BF, P>
where
    BF: FieldVar<P::BaseField>,
    for<'a> &'a BF: FieldOpsBounds<'a, P::BaseField, BF>,
    P: CubicExtVarParams<BF>,
{
    fn cs(&self) -> Option<ConstraintSystemRef<BF::ConstraintF>> {
        [&self.c0, &self.c1, &self.c2].cs()
    }
}

impl<BF, P> From<Boolean<BF::ConstraintF>> for CubicExtVar<BF, P>
where
    BF: FieldVar<P::BaseField>,
    for<'a> &'a BF: FieldOpsBounds<'a, P::BaseField, BF>,
    P: CubicExtVarParams<BF>,
{
    fn from(other: Boolean<BF::ConstraintF>) -> Self {
        let c0 = BF::from(other);
        let c1 = BF::zero();
        let c2 = BF::zero();
        Self::new(c0, c1, c2)
    }
}

impl<'a, BF, P> FieldOpsBounds<'a, CubicExtField<P>, CubicExtVar<BF, P>> for CubicExtVar<BF, P>
where
    BF: FieldVar<P::BaseField>,
    for<'b> &'b BF: FieldOpsBounds<'b, P::BaseField, BF>,
    P: CubicExtVarParams<BF>,
{
}
impl<'a, BF, P> FieldOpsBounds<'a, CubicExtField<P>, CubicExtVar<BF, P>> for &'a CubicExtVar<BF, P>
where
    BF: FieldVar<P::BaseField>,
    for<'b> &'b BF: FieldOpsBounds<'b, P::BaseField, BF>,
    P: CubicExtVarParams<BF>,
{
}

impl<BF, P> FieldVar<CubicExtField<P>> for CubicExtVar<BF, P>
where
    BF: FieldVar<P::BaseField>,
    for<'a> &'a BF: FieldOpsBounds<'a, P::BaseField, BF>,
    P: CubicExtVarParams<BF>,
{
    type ConstraintF = BF::ConstraintF;

    fn constant(other: CubicExtField<P>) -> Self {
        let c0 = BF::constant(other.c0);
        let c1 = BF::constant(other.c1);
        let c2 = BF::constant(other.c2);
        Self::new(c0, c1, c2)
    }

    fn zero() -> Self {
        let c0 = BF::zero();
        let c1 = BF::zero();
        let c2 = BF::zero();
        Self::new(c0, c1, c2)
    }

    fn one() -> Self {
        let c0 = BF::one();
        let c1 = BF::zero();
        let c2 = BF::zero();
        Self::new(c0, c1, c2)
    }

    #[inline]
    fn value(&self) -> Result<CubicExtField<P>, SynthesisError> {
        match (self.c0.value(), self.c1.value(), self.c2.value()) {
            (Ok(c0), Ok(c1), Ok(c2)) => Ok(CubicExtField::new(c0, c1, c2)),
            (..) => Err(SynthesisError::AssignmentMissing),
        }
    }

    #[inline]
    fn double(&self) -> Result<Self, SynthesisError> {
        let c0 = self.c0.double()?;
        let c1 = self.c1.double()?;
        let c2 = self.c2.double()?;
        Ok(Self::new(c0, c1, c2))
    }

    #[inline]
    fn negate(&self) -> Result<Self, SynthesisError> {
        let mut result = self.clone();
        result.c0.negate_in_place()?;
        result.c1.negate_in_place()?;
        result.c2.negate_in_place()?;
        Ok(result)
    }

    /// Use the Chung-Hasan asymmetric squaring formula.
    ///
    /// (Devegili OhEig Scott Dahab --- Multiplication and Squaring on
    /// Abstract Pairing-Friendly
    /// Fields.pdf; Section 4 (CH-SQR2))
    #[inline]
    fn square(&self) -> Result<Self, SynthesisError> {
        let a = self.c0.clone();
        let b = self.c1.clone();
        let c = self.c2.clone();

        let s0 = a.square()?;
        let ab = &a * &b;
        let s1 = ab.double()?;
        let s2 = (&a - &b + &c).square()?;
        let s3 = (&b * &c).double()?;
        let s4 = c.square()?;

        let c0 = Self::mul_base_field_by_nonresidue(&s3)? + &s0;
        let c1 = Self::mul_base_field_by_nonresidue(&s4)? + &s1;
        let c2 = s1 + &s2 + &s3 - &s0 - &s4;

        Ok(Self::new(c0, c1, c2))
    }

    fn mul_equals(&self, other: &Self, result: &Self) -> Result<(), SynthesisError> {
        // Karatsuba multiplication for cubic extensions:
        //     v0 = A.c0 * B.c0
        //     v1 = A.c1 * B.c1
        //     v2 = A.c2 * B.c2
        //     result.c0 = v0 + β((a1 + a2)(b1 + b2) − v1 − v2)
        //     result.c1 = (a0 + a1)(b0 + b1) − v0 − v1 + βv2
        //     result.c2 = (a0 + a2)(b0 + b2) − v0 + v1 − v2,
        // We enforce this with six constraints:
        //
        //     v0 = A.c0 * B.c0
        //     v1 = A.c1 * B.c1
        //     v2 = A.c2 * B.c2
        //
        //     result.c0 - v0 + \beta*(v1 + v2) = β(a1 + a2)(b1 + b2))
        //     result.c1 + v0 + v1 - βv2        = (a0 + a1)(b0 + b1)
        //     result.c2 + v0 - v1 + v2         = (a0 + a2)(b0 + b2)
        // Reference:
        // "Multiplication and Squaring on Pairing-Friendly Fields"
        // Devegili, OhEigeartaigh, Scott, Dahab
        //
        // This implementation adapted from
        // https://github.com/ZencashOfficial/ginger-lib/blob/development/r1cs/gadgets/std/src/fields/fp3.rs
        let v0 = &self.c0 * &other.c0;
        let v1 = &self.c1 * &other.c1;
        let v2 = &self.c2 * &other.c2;

        // Check c0
        let nr_a1_plus_a2 = (&self.c1 + &self.c2) * P::NONRESIDUE;
        let b1_plus_b2 = &other.c1 + &other.c2;
        let nr_v1 = &v1 * P::NONRESIDUE;
        let nr_v2 = &v2 * P::NONRESIDUE;
        let to_check = &result.c0 - &v0 + &nr_v1 + &nr_v2;
        nr_a1_plus_a2.mul_equals(&b1_plus_b2, &to_check)?;

        // Check c1
        let a0_plus_a1 = &self.c0 + &self.c1;
        let b0_plus_b1 = &other.c0 + &other.c1;
        let to_check = &result.c1 - &nr_v2 + &v0 + &v1;
        a0_plus_a1.mul_equals(&b0_plus_b1, &to_check)?;

        // Check c2
        let a0_plus_a2 = &self.c0 + &self.c2;
        let b0_plus_b2 = &other.c0 + &other.c2;
        let to_check = &result.c2 + &v0 - &v1 + &v2;
        a0_plus_a2.mul_equals(&b0_plus_b2, &to_check)?;
        Ok(())
    }

    fn frobenius_map(&self, power: usize) -> Result<Self, SynthesisError> {
        let mut result = self.clone();
        result.c0.frobenius_map_in_place(power)?;
        result.c1.frobenius_map_in_place(power)?;
        result.c2.frobenius_map_in_place(power)?;

        P::mul_base_field_vars_by_frob_coeff(&mut result.c1, &mut result.c2, power);
        Ok(result)
    }

    fn inverse(&self) -> Result<Self, SynthesisError> {
        let cs = self.cs().get()?.clone();
        let one = Self::new_constant(cs.clone(), CubicExtField::one())?;
        let inverse = Self::new_witness(cs, || self.value().and_then(|v| v.inverse().get()))?;
        self.mul_equals(&inverse, &one)?;
        Ok(inverse)
    }
}

impl_bounded_ops!(
    CubicExtVar<BF, P>,
    CubicExtField<P>,
    Add,
    add,
    AddAssign,
    add_assign,
    |this: &'a CubicExtVar<BF, P>, other: &'a CubicExtVar<BF, P>| {
        let c0 = &this.c0 + &other.c0;
        let c1 = &this.c1 + &other.c1;
        let c2 = &this.c2 + &other.c2;
        CubicExtVar::new(c0, c1, c2)
    },
    |this: &'a CubicExtVar<BF, P>, other: CubicExtField<P>| {
        this + CubicExtVar::constant(other)
    },
    (BF: FieldVar<P::BaseField>, P: CubicExtVarParams<BF>),
    for<'b> &'b BF: FieldOpsBounds<'b, P::BaseField, BF>,
);
impl_bounded_ops!(
    CubicExtVar<BF, P>,
    CubicExtField<P>,
    Sub,
    sub,
    SubAssign,
    sub_assign,
    |this: &'a CubicExtVar<BF, P>, other: &'a CubicExtVar<BF, P>| {
        let c0 = &this.c0 - &other.c0;
        let c1 = &this.c1 - &other.c1;
        let c2 = &this.c2 - &other.c2;
        CubicExtVar::new(c0, c1, c2)
    },
    |this: &'a CubicExtVar<BF, P>, other: CubicExtField<P>| {
        this - CubicExtVar::constant(other)
    },
    (BF: FieldVar<P::BaseField>, P: CubicExtVarParams<BF>),
    for<'b> &'b BF: FieldOpsBounds<'b, P::BaseField, BF>,
);
impl_bounded_ops!(
    CubicExtVar<BF, P>,
    CubicExtField<P>,
    Mul,
    mul,
    MulAssign,
    mul_assign,
    |this: &'a CubicExtVar<BF, P>, other: &'a CubicExtVar<BF, P>| {
        // Karatsuba multiplication for cubic extensions:
        //     v0 = A.c0 * B.c0
        //     v1 = A.c1 * B.c1
        //     v2 = A.c2 * B.c2
        //     result.c0 = v0 + β((a1 + a2)(b1 + b2) − v1 − v2)
        //     result.c1 = (a0 + a1)(b0 + b1) − v0 − v1 + βv2
        //     result.c2 = (a0 + a2)(b0 + b2) − v0 + v1 − v2,
        //
        // Reference:
        // "Multiplication and Squaring on Pairing-Friendly Fields"
        // Devegili, OhEigeartaigh, Scott, Dahab
        let v0 = &this.c0 * &other.c0;
        let v1 = &this.c1 * &other.c1;
        let v2 = &this.c2 * &other.c2;
        let c0 =
            (((&this.c1 + &this.c2) * (&other.c1 + &other.c2) - &v1 - &v2) * P::NONRESIDUE) + &v0 ;
        let c1 =
            (&this.c0 + &this.c1) * (&other.c0 + &other.c1) - &v0 - &v1 + (&v2 * P::NONRESIDUE);
        let c2 =
            (&this.c0 + &this.c2) * (&other.c0 + &other.c2) - &v0 + &v1 - &v2;

        CubicExtVar::new(c0, c1, c2)
    },
    |this: &'a CubicExtVar<BF, P>, other: CubicExtField<P>| {
        this * CubicExtVar::constant(other)
    },
    (BF: FieldVar<P::BaseField>, P: CubicExtVarParams<BF>),
    for<'b> &'b BF: FieldOpsBounds<'b, P::BaseField, BF>,
);

impl<BF, P> EqGadget<BF::ConstraintF> for CubicExtVar<BF, P>
where
    BF: FieldVar<P::BaseField>,
    for<'a> &'a BF: FieldOpsBounds<'a, P::BaseField, BF>,
    P: CubicExtVarParams<BF>,
{
    fn is_eq(&self, other: &Self) -> Result<Boolean<BF::ConstraintF>, SynthesisError> {
        let b0 = self.c0.is_eq(&other.c0)?;
        let b1 = self.c1.is_eq(&other.c1)?;
        let b2 = self.c2.is_eq(&other.c2)?;
        b0.and(&b1)?.and(&b2)
    }

    #[inline]
    fn conditional_enforce_equal(
        &self,
        other: &Self,
        condition: &Boolean<BF::ConstraintF>,
    ) -> Result<(), SynthesisError> {
        self.c0.conditional_enforce_equal(&other.c0, condition)?;
        self.c1.conditional_enforce_equal(&other.c1, condition)?;
        self.c2.conditional_enforce_equal(&other.c2, condition)?;
        Ok(())
    }

    #[inline]
    fn conditional_enforce_not_equal(
        &self,
        other: &Self,
        condition: &Boolean<BF::ConstraintF>,
    ) -> Result<(), SynthesisError> {
        let is_equal = self.is_eq(other)?;
        is_equal
            .and(condition)?
            .enforce_equal(&Boolean::Constant(false))
    }
}

impl<BF, P> ToBitsGadget<BF::ConstraintF> for CubicExtVar<BF, P>
where
    BF: FieldVar<P::BaseField>,
    for<'a> &'a BF: FieldOpsBounds<'a, P::BaseField, BF>,
    P: CubicExtVarParams<BF>,
{
    fn to_bits(&self) -> Result<Vec<Boolean<BF::ConstraintF>>, SynthesisError> {
        let mut c0 = self.c0.to_bits()?;
        let mut c1 = self.c1.to_bits()?;
        let mut c2 = self.c2.to_bits()?;
        c0.append(&mut c1);
        c0.append(&mut c2);
        Ok(c0)
    }

    fn to_non_unique_bits(&self) -> Result<Vec<Boolean<BF::ConstraintF>>, SynthesisError> {
        let mut c0 = self.c0.to_non_unique_bits()?;
        let mut c1 = self.c1.to_non_unique_bits()?;
        let mut c2 = self.c2.to_non_unique_bits()?;
        c0.append(&mut c1);
        c0.append(&mut c2);
        Ok(c0)
    }
}

impl<BF, P> ToBytesGadget<BF::ConstraintF> for CubicExtVar<BF, P>
where
    BF: FieldVar<P::BaseField>,
    for<'a> &'a BF: FieldOpsBounds<'a, P::BaseField, BF>,
    P: CubicExtVarParams<BF>,
{
    fn to_bytes(&self) -> Result<Vec<UInt8<BF::ConstraintF>>, SynthesisError> {
        let mut c0 = self.c0.to_bytes()?;
        let mut c1 = self.c1.to_bytes()?;
        let mut c2 = self.c2.to_bytes()?;
        c0.append(&mut c1);
        c0.append(&mut c2);

        Ok(c0)
    }

    fn to_non_unique_bytes(&self) -> Result<Vec<UInt8<BF::ConstraintF>>, SynthesisError> {
        let mut c0 = self.c0.to_non_unique_bytes()?;
        let mut c1 = self.c1.to_non_unique_bytes()?;
        let mut c2 = self.c2.to_non_unique_bytes()?;

        c0.append(&mut c1);
        c0.append(&mut c2);

        Ok(c0)
    }
}

impl<BF, P> CondSelectGadget<BF::ConstraintF> for CubicExtVar<BF, P>
where
    BF: FieldVar<P::BaseField>,
    for<'a> &'a BF: FieldOpsBounds<'a, P::BaseField, BF>,
    P: CubicExtVarParams<BF>,
{
    #[inline]
    fn conditionally_select(
        cond: &Boolean<BF::ConstraintF>,
        true_value: &Self,
        false_value: &Self,
    ) -> Result<Self, SynthesisError> {
        let c0 = BF::conditionally_select(cond, &true_value.c0, &false_value.c0)?;
        let c1 = BF::conditionally_select(cond, &true_value.c1, &false_value.c1)?;
        let c2 = BF::conditionally_select(cond, &true_value.c2, &false_value.c2)?;
        Ok(Self::new(c0, c1, c2))
    }
}

impl<BF, P> TwoBitLookupGadget<BF::ConstraintF> for CubicExtVar<BF, P>
where
    BF: FieldVar<P::BaseField>
        + TwoBitLookupGadget<
            <BF as FieldVar<P::BaseField>>::ConstraintF,
            TableConstant = P::BaseField,
        >,
    for<'a> &'a BF: FieldOpsBounds<'a, P::BaseField, BF>,
    P: CubicExtVarParams<BF>,
{
    type TableConstant = CubicExtField<P>;

    fn two_bit_lookup(
        b: &[Boolean<BF::ConstraintF>],
        c: &[Self::TableConstant],
    ) -> Result<Self, SynthesisError> {
        let c0s = c.iter().map(|f| f.c0).collect::<Vec<_>>();
        let c1s = c.iter().map(|f| f.c1).collect::<Vec<_>>();
        let c2s = c.iter().map(|f| f.c2).collect::<Vec<_>>();
        let c0 = BF::two_bit_lookup(b, &c0s)?;
        let c1 = BF::two_bit_lookup(b, &c1s)?;
        let c2 = BF::two_bit_lookup(b, &c2s)?;
        Ok(Self::new(c0, c1, c2))
    }
}

impl<BF, P> ThreeBitCondNegLookupGadget<BF::ConstraintF> for CubicExtVar<BF, P>
where
    BF: FieldVar<P::BaseField>
        + ThreeBitCondNegLookupGadget<
            <BF as FieldVar<P::BaseField>>::ConstraintF,
            TableConstant = P::BaseField,
        >,
    for<'a> &'a BF: FieldOpsBounds<'a, P::BaseField, BF>,
    P: CubicExtVarParams<BF>,
{
    type TableConstant = CubicExtField<P>;

    fn three_bit_cond_neg_lookup(
        b: &[Boolean<BF::ConstraintF>],
        b0b1: &Boolean<BF::ConstraintF>,
        c: &[Self::TableConstant],
    ) -> Result<Self, SynthesisError> {
        let c0s = c.iter().map(|f| f.c0).collect::<Vec<_>>();
        let c1s = c.iter().map(|f| f.c1).collect::<Vec<_>>();
        let c2s = c.iter().map(|f| f.c2).collect::<Vec<_>>();
        let c0 = BF::three_bit_cond_neg_lookup(b, b0b1, &c0s)?;
        let c1 = BF::three_bit_cond_neg_lookup(b, b0b1, &c1s)?;
        let c2 = BF::three_bit_cond_neg_lookup(b, b0b1, &c2s)?;
        Ok(Self::new(c0, c1, c2))
    }
}

impl<BF, P> AllocVar<CubicExtField<P>, BF::ConstraintF> for CubicExtVar<BF, P>
where
    BF: FieldVar<P::BaseField>,
    for<'a> &'a BF: FieldOpsBounds<'a, P::BaseField, BF>,
    P: CubicExtVarParams<BF>,
{
    fn new_variable<T: Borrow<CubicExtField<P>>>(
        cs: impl Into<Namespace<BF::ConstraintF>>,
        f: impl FnOnce() -> Result<T, SynthesisError>,
        mode: AllocationMode,
    ) -> Result<Self, SynthesisError> {
        let ns = cs.into();
        let cs = ns.cs();

        use SynthesisError::*;
        let (c0, c1, c2) = match f() {
            Ok(fe) => (Ok(fe.borrow().c0), Ok(fe.borrow().c1), Ok(fe.borrow().c2)),
            Err(_) => (
                Err(AssignmentMissing),
                Err(AssignmentMissing),
                Err(AssignmentMissing),
            ),
        };

        let c0 = BF::new_variable(cs.ns("c0"), || c0, mode)?;
        let c1 = BF::new_variable(cs.ns("c1"), || c1, mode)?;
        let c2 = BF::new_variable(cs.ns("c2"), || c2, mode)?;
        Ok(Self::new(c0, c1, c2))
    }
}
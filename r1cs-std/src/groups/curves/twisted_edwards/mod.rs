use algebra::{
    curves::{
        twisted_edwards_extended::{GroupAffine as TEAffine, GroupProjective as TEProjective},
        AffineCurve, MontgomeryModelParameters, ProjectiveCurve, TEModelParameters,
    },
    BigInteger, BitIterator, Field, One, PrimeField, Zero,
};

use r1cs_core::{ConstraintSystemRef, Namespace, SynthesisError};

use crate::{prelude::*, Vec};

use core::{borrow::Borrow, marker::PhantomData};

#[derive(Derivative)]
#[derivative(Debug, Clone)]
#[must_use]
pub struct MontgomeryAffineVar<P: TEModelParameters, F: FieldVar<P::BaseField>>
where
    for<'a> &'a F: FieldOpsBounds<'a, P::BaseField, F>,
{
    pub x: F,
    pub y: F,
    #[derivative(Debug = "ignore")]
    _params: PhantomData<P>,
}

mod montgomery_affine_impl {
    use super::*;
    use algebra::{twisted_edwards_extended::GroupAffine, Field};
    use core::ops::Add;

    impl<P, F> R1CSVar<F::ConstraintF> for MontgomeryAffineVar<P, F>
    where
        P: TEModelParameters,
        F: FieldVar<P::BaseField>,
        for<'a> &'a F: FieldOpsBounds<'a, P::BaseField, F>,
    {
        fn cs(&self) -> Option<ConstraintSystemRef<F::ConstraintF>> {
            self.x.cs().or(self.y.cs())
        }
    }

    impl<P: TEModelParameters, F: FieldVar<P::BaseField>> MontgomeryAffineVar<P, F>
    where
        for<'a> &'a F: FieldOpsBounds<'a, P::BaseField, F>,
    {
        pub fn new(x: F, y: F) -> Self {
            Self {
                x,
                y,
                _params: PhantomData,
            }
        }

        pub fn from_edwards_to_coords(
            p: &TEAffine<P>,
        ) -> Result<(P::BaseField, P::BaseField), SynthesisError> {
            let montgomery_point: GroupAffine<P> = if p.y == P::BaseField::one() {
                GroupAffine::zero()
            } else if p.x == P::BaseField::zero() {
                GroupAffine::new(P::BaseField::zero(), P::BaseField::zero())
            } else {
                let u =
                    (P::BaseField::one() + &p.y) * &(P::BaseField::one() - &p.y).inverse().unwrap();
                let v = u * &p.x.inverse().unwrap();
                GroupAffine::new(u, v)
            };

            Ok((montgomery_point.x, montgomery_point.y))
        }

        pub fn new_witness_from_edwards(
            cs: ConstraintSystemRef<F::ConstraintF>,
            p: &TEAffine<P>,
        ) -> Result<Self, SynthesisError> {
            let montgomery_coords = Self::from_edwards_to_coords(p)?;
            let u = F::new_witness(cs.ns("u"), || Ok(montgomery_coords.0))?;
            let v = F::new_witness(cs.ns("v"), || Ok(montgomery_coords.1))?;
            Ok(Self::new(u, v))
        }

        pub fn into_edwards(&self) -> Result<AffineVar<P, F>, SynthesisError> {
            let cs = self.cs().unwrap_or(ConstraintSystemRef::None);
            // Compute u = x / y
            let u = F::new_witness(cs.ns("u"), || {
                let y_inv = self
                    .y
                    .value()?
                    .inverse()
                    .ok_or(SynthesisError::DivisionByZero)?;
                Ok(self.x.value()? * &y_inv)
            })?;

            u.mul_equals(&self.y, &self.x)?;

            let v = F::new_witness(cs.ns("v"), || {
                let mut t0 = self.x.value()?;
                let mut t1 = t0;
                t0 -= &P::BaseField::one();
                t1 += &P::BaseField::one();

                Ok(t0 * &t1.inverse().ok_or(SynthesisError::DivisionByZero)?)
            })?;

            let xplusone = &self.x + P::BaseField::one();
            let xminusone = &self.x - P::BaseField::one();
            v.mul_equals(&xplusone, &xminusone)?;

            Ok(AffineVar::new(u, v))
        }
    }

    impl<'a, P, F> Add<&'a MontgomeryAffineVar<P, F>> for MontgomeryAffineVar<P, F>
    where
        P: TEModelParameters,
        F: FieldVar<P::BaseField>,
        for<'b> &'b F: FieldOpsBounds<'b, P::BaseField, F>,
    {
        type Output = MontgomeryAffineVar<P, F>;
        fn add(self, other: &'a Self) -> Self::Output {
            let cs = [&self, other].cs();
            let mode = if cs.is_none() || matches!(cs, Some(ConstraintSystemRef::None)) {
                AllocationMode::Constant
            } else {
                AllocationMode::Witness
            };
            let cs = cs.unwrap_or(ConstraintSystemRef::None);

            let coeff_b = P::MontgomeryModelParameters::COEFF_B;
            let coeff_a = P::MontgomeryModelParameters::COEFF_A;

            let lambda = F::new_variable(
                cs.ns("lambda"),
                || {
                    let n = other.y.value()? - &self.y.value()?;
                    let d = other.x.value()? - &self.x.value()?;
                    Ok(n * &d.inverse().ok_or(SynthesisError::DivisionByZero)?)
                },
                mode,
            )
            .unwrap();
            let lambda_n = &other.y - &self.y;
            let lambda_d = &other.x - &self.x;
            lambda_d.mul_equals(&lambda, &lambda_n).unwrap();

            // Compute x'' = B*lambda^2 - A - x - x'
            let xprime = F::new_variable(
                cs.ns("xprime"),
                || {
                    Ok(lambda.value()?.square() * &coeff_b
                        - &coeff_a
                        - &self.x.value()?
                        - &other.x.value()?)
                },
                mode,
            )
            .unwrap();

            let xprime_lc = &self.x + &other.x + &xprime + coeff_a;
            // (lambda) * (lambda) = (A + x + x' + x'')
            let lambda_b = &lambda * coeff_b;
            lambda_b.mul_equals(&lambda, &xprime_lc).unwrap();

            let yprime = F::new_variable(
                cs.ns("yprime"),
                || {
                    Ok(-(self.y.value()?
                        + &(lambda.value()? * &(xprime.value()? - &self.x.value()?))))
                },
                mode,
            )
            .unwrap();

            let xres = &self.x - &xprime;
            let yres = &self.y + &yprime;
            lambda.mul_equals(&xres, &yres).unwrap();
            MontgomeryAffineVar::new(xprime, yprime)
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug, Clone)]
#[must_use]
pub struct AffineVar<P: TEModelParameters, F: FieldVar<P::BaseField>>
where
    for<'a> &'a F: FieldOpsBounds<'a, P::BaseField, F>,
{
    pub x: F,
    pub y: F,
    #[derivative(Debug = "ignore")]
    _params: PhantomData<P>,
}

impl<P: TEModelParameters, F: FieldVar<P::BaseField>> AffineVar<P, F>
where
    for<'a> &'a F: FieldOpsBounds<'a, P::BaseField, F>,
{
    pub fn new(x: F, y: F) -> Self {
        Self {
            x,
            y,
            _params: PhantomData,
        }
    }

    /// Allocates a new variable without performing an on-curve check, which is
    /// useful if the variable is known to be on the curve (eg., if the point
    /// is a constant or is a public input).
    pub fn new_variable_omit_on_curve_check<T: Into<TEAffine<P>>>(
        cs: impl Into<Namespace<F::ConstraintF>>,
        f: impl FnOnce() -> Result<T, SynthesisError>,
        mode: AllocationMode,
    ) -> Result<Self, SynthesisError> {
        let ns = cs.into();
        let cs = ns.cs();

        let (x, y) = match f() {
            Ok(ge) => {
                let ge: TEAffine<P> = ge.into();
                (Ok(ge.x), Ok(ge.y))
            }
            _ => (
                Err(SynthesisError::AssignmentMissing),
                Err(SynthesisError::AssignmentMissing),
            ),
        };

        let x = F::new_variable(cs.ns("x"), || x, mode)?;
        let y = F::new_variable(cs.ns("y"), || y, mode)?;

        Ok(Self::new(x, y))
    }
}

impl<P, F> R1CSVar<F::ConstraintF> for AffineVar<P, F>
where
    P: TEModelParameters,
    F: FieldVar<P::BaseField>,
    for<'a> &'a F: FieldOpsBounds<'a, P::BaseField, F>,
{
    fn cs(&self) -> Option<ConstraintSystemRef<F::ConstraintF>> {
        self.x.cs().or(self.y.cs())
    }
}

impl<P, F> GroupVar<TEProjective<P>> for AffineVar<P, F>
where
    P: TEModelParameters,
    F: FieldVar<P::BaseField>
        + TwoBitLookupGadget<<F as FieldVar<P::BaseField>>::ConstraintF, TableConstant = P::BaseField>
        + ThreeBitCondNegLookupGadget<
            <F as FieldVar<P::BaseField>>::ConstraintF,
            TableConstant = P::BaseField,
        >,
    for<'a> &'a F: FieldOpsBounds<'a, P::BaseField, F>,
{
    type ConstraintF = F::ConstraintF;

    fn constant(g: TEProjective<P>) -> Self {
        let cs = ConstraintSystemRef::None;
        Self::new_variable_omit_on_curve_check(cs, || Ok(g), AllocationMode::Constant).unwrap()
    }

    fn zero() -> Self {
        Self::new(F::zero(), F::one())
    }

    fn is_zero(&self) -> Result<Boolean<Self::ConstraintF>, SynthesisError> {
        self.x.is_zero()?.and(&self.x.is_one()?)
    }

    #[inline]
    fn value(&self) -> Result<TEProjective<P>, SynthesisError> {
        let (x, y) = (self.x.value()?, self.y.value()?);
        let result = TEAffine::new(x, y);
        Ok(result.into())
    }

    fn new_variable_omit_prime_order_check(
        cs: impl Into<Namespace<Self::ConstraintF>>,
        f: impl FnOnce() -> Result<TEProjective<P>, SynthesisError>,
        mode: AllocationMode,
    ) -> Result<Self, SynthesisError> {
        let ns = cs.into();
        let cs = ns.cs();

        let g = Self::new_variable_omit_on_curve_check(cs, f, mode)?;

        if mode != AllocationMode::Constant {
            let d = P::COEFF_D;
            let a = P::COEFF_A;
            // Check that ax^2 + y^2 = 1 + dx^2y^2
            // We do this by checking that ax^2 - 1 = y^2 * (dx^2 - 1)
            let x2 = g.x.square()?;
            let y2 = g.y.square()?;

            let one = P::BaseField::one();
            let d_x2_minus_one = &x2 * d - one;
            let a_x2_minus_one = &x2 * a - one;

            d_x2_minus_one.mul_equals(&y2, &a_x2_minus_one)?;
        }
        Ok(g)
    }

    /// Enforce that `self` is in the prime-order subgroup.
    ///
    /// Does so by multiplying by the prime order, and checking that the result
    /// is unchanged.
    fn enforce_prime_order(&self) -> Result<(), SynthesisError> {
        let r_minus_1 = (-P::ScalarField::one()).into_repr();

        let mut seen_one = false;
        let mut result = Self::zero();
        for b in BitIterator::new(r_minus_1) {
            let old_seen_one = seen_one;
            if seen_one {
                result.double_in_place()?;
            } else {
                seen_one = b;
            }

            if b {
                result = if old_seen_one {
                    result + self
                } else {
                    self.clone()
                };
            }
        }
        self.negate()?.enforce_equal(&result)?;
        Ok(())
    }

    #[inline]
    fn double_in_place(&mut self) -> Result<(), SynthesisError> {
        if let Some(cs) = self.cs() {
            let a = P::COEFF_A;

            // xy
            let xy = &self.x * &self.y;
            let x2 = self.x.square()?;
            let y2 = self.y.square()?;

            let a_x2 = &x2 * a;

            // Compute x3 = (2xy) / (ax^2 + y^2)
            let x3 = F::new_witness(cs.ns("x3"), || {
                let t0 = xy.value()?.double();
                let t1 = a * &x2.value()? + &y2.value()?;
                Ok(t0 * &t1.inverse().ok_or(SynthesisError::DivisionByZero)?)
            })?;

            let a_x2_plus_y2 = &a_x2 + &y2;
            let two_xy = xy.double()?;
            x3.mul_equals(&a_x2_plus_y2, &two_xy)?;

            // Compute y3 = (y^2 - ax^2) / (2 - ax^2 - y^2)
            let two = P::BaseField::one().double();
            let y3 = F::new_witness(cs.ns("y3"), || {
                let a_x2 = a * &x2.value()?;
                let t0 = y2.value()? - &a_x2;
                let t1 = two - &a_x2 - &y2.value()?;
                Ok(t0 * &t1.inverse().ok_or(SynthesisError::DivisionByZero)?)
            })?;
            let y2_minus_a_x2 = &y2 - &a_x2;
            let two_minus_ax2_minus_y2 = (&a_x2 + &y2).negate()? + two;

            y3.mul_equals(&two_minus_ax2_minus_y2, &y2_minus_a_x2)?;
            self.x = x3;
            self.y = y3;
        } else {
            let value = self.value()?;
            *self = Self::constant(value.double());
        }
        Ok(())
    }

    fn negate(&self) -> Result<Self, SynthesisError> {
        Ok(Self::new(self.x.negate()?, self.y.clone()))
    }

    fn precomputed_base_scalar_mul<'a, I, B>(
        &mut self,
        scalar_bits_with_base_powers: I,
    ) -> Result<(), SynthesisError>
    where
        I: Iterator<Item = (B, &'a TEProjective<P>)>,
        B: Borrow<Boolean<Self::ConstraintF>>,
    {
        let scalar_bits_with_base_powers = scalar_bits_with_base_powers
            .map(|(bit, base)| (bit.borrow().clone(), (*base).into()))
            .collect::<Vec<(_, TEProjective<P>)>>();
        let zero = TEProjective::zero();
        for bits_base_powers in scalar_bits_with_base_powers.chunks(2) {
            if bits_base_powers.len() == 2 {
                let bits = [bits_base_powers[0].0.clone(), bits_base_powers[1].0.clone()];
                let base_powers = [&bits_base_powers[0].1, &bits_base_powers[1].1];

                let mut table = [
                    zero,
                    *base_powers[0],
                    *base_powers[1],
                    *base_powers[0] + base_powers[1],
                ];

                TEProjective::batch_normalization(&mut table);
                let x_s = [table[0].x, table[1].x, table[2].x, table[3].x];
                let y_s = [table[0].y, table[1].y, table[2].y, table[3].y];

                let x = F::two_bit_lookup(&bits, &x_s)?;
                let y = F::two_bit_lookup(&bits, &y_s)?;
                *self += Self::new(x, y);
            } else if bits_base_powers.len() == 1 {
                let bit = bits_base_powers[0].0.clone();
                let base_power = bits_base_powers[0].1;
                let new_encoded = &*self + base_power;
                *self = bit.select(&new_encoded, &self)?;
            }
        }

        Ok(())
    }

    fn precomputed_base_3_bit_signed_digit_scalar_mul<'a, I, J, B>(
        bases: &[B],
        scalars: &[J],
    ) -> Result<Self, SynthesisError>
    where
        I: Borrow<[Boolean<F::ConstraintF>]>,
        J: Borrow<[I]>,
        B: Borrow<[TEProjective<P>]>,
    {
        const CHUNK_SIZE: usize = 3;
        let mut ed_result: Option<AffineVar<P, F>> = None;
        let mut result: Option<MontgomeryAffineVar<P, F>> = None;

        let mut process_segment_result = |result: &MontgomeryAffineVar<P, F>| {
            let sgmt_result = result.into_edwards()?;
            ed_result = match ed_result.as_ref() {
                None => Some(sgmt_result),
                Some(r) => Some(sgmt_result + r),
            };
            Ok::<(), SynthesisError>(())
        };

        // Compute ∏(h_i^{m_i}) for all i.
        for (segment_bits_chunks, segment_powers) in scalars.iter().zip(bases) {
            for (bits, base_power) in segment_bits_chunks
                .borrow()
                .iter()
                .zip(segment_powers.borrow())
            {
                let base_power = base_power.borrow();
                let mut acc_power = *base_power;
                let mut coords = vec![];
                for _ in 0..4 {
                    coords.push(acc_power);
                    acc_power += base_power;
                }

                let bits = bits.borrow().to_bits()?;
                if bits.len() != CHUNK_SIZE {
                    return Err(SynthesisError::Unsatisfiable);
                }

                let coords = coords
                    .iter()
                    .map(|p| MontgomeryAffineVar::from_edwards_to_coords(&p.into_affine()))
                    .collect::<Result<Vec<_>, _>>()?;

                let x_coeffs = coords.iter().map(|p| p.0).collect::<Vec<_>>();
                let y_coeffs = coords.iter().map(|p| p.1).collect::<Vec<_>>();

                let precomp = bits[0].and(&bits[1])?;

                let x = F::zero()
                    + x_coeffs[0]
                    + F::from(bits[0].clone()) * (x_coeffs[1] - &x_coeffs[0])
                    + F::from(bits[1].clone()) * (x_coeffs[2] - &x_coeffs[0])
                    + F::from(precomp.clone())
                        * (x_coeffs[3] - &x_coeffs[2] - &x_coeffs[1] + &x_coeffs[0]);

                let y = F::three_bit_cond_neg_lookup(&bits, &precomp, &y_coeffs)?;

                let tmp = MontgomeryAffineVar::new(x, y);
                result = match result.as_ref() {
                    None => Some(tmp),
                    Some(r) => Some(tmp + r),
                };
            }

            process_segment_result(&result.unwrap())?;
            result = None;
        }
        if result.is_some() {
            process_segment_result(&result.unwrap())?;
        }
        Ok(ed_result.unwrap())
    }
}

impl<P, F> AllocVar<TEProjective<P>, F::ConstraintF> for AffineVar<P, F>
where
    P: TEModelParameters,
    F: FieldVar<P::BaseField>
        + TwoBitLookupGadget<<F as FieldVar<P::BaseField>>::ConstraintF, TableConstant = P::BaseField>
        + ThreeBitCondNegLookupGadget<
            <F as FieldVar<P::BaseField>>::ConstraintF,
            TableConstant = P::BaseField,
        >,
    for<'a> &'a F: FieldOpsBounds<'a, P::BaseField, F>,
{
    fn new_variable<Point: Borrow<TEProjective<P>>>(
        cs: impl Into<Namespace<F::ConstraintF>>,
        f: impl FnOnce() -> Result<Point, SynthesisError>,
        mode: AllocationMode,
    ) -> Result<Self, SynthesisError> {
        let ns = cs.into();
        let cs = ns.cs();
        let f = || Ok(*f()?.borrow());
        match mode {
            AllocationMode::Constant => Self::new_variable_omit_prime_order_check(cs, f, mode),
            AllocationMode::Input => Self::new_variable_omit_prime_order_check(cs, f, mode),
            AllocationMode::Witness => {
                // if cofactor.is_even():
                //   divide until you've removed all even factors
                // else:
                //   just directly use double and add.
                let mut power_of_2: u32 = 0;
                let mut cofactor = P::COFACTOR.to_vec();
                while cofactor[0] % 2 == 0 {
                    div2(&mut cofactor);
                    power_of_2 += 1;
                }

                let cofactor_weight = BitIterator::new(cofactor.as_slice()).filter(|b| *b).count();
                let modulus_minus_1 = (-P::ScalarField::one()).into_repr(); // r - 1
                let modulus_minus_1_weight =
                    BitIterator::new(modulus_minus_1).filter(|b| *b).count();

                // We pick the most efficient method of performing the prime order check:
                // If the cofactor has lower hamming weight than the scalar field's modulus,
                // we first multiply by the inverse of the cofactor, and then, after allocating,
                // multiply by the cofactor. This ensures the resulting point has no cofactors
                //
                // Else, we multiply by the scalar field's modulus and ensure that the result
                // equals the identity.

                let (mut ge, iter) = if cofactor_weight < modulus_minus_1_weight {
                    let ge = Self::new_variable_omit_prime_order_check(
                        cs.ns("Witness without subgroup check with cofactor mul"),
                        || f().map(|g| g.borrow().into_affine().mul_by_cofactor_inv().into()),
                        mode,
                    )?;
                    (ge, BitIterator::new(cofactor.as_slice()))
                } else {
                    let ge = Self::new_variable_omit_prime_order_check(
                        cs.ns("Witness without subgroup check with `r` check"),
                        || {
                            f().map(|g| {
                                let g = g.into_affine();
                                let mut power_of_two = P::ScalarField::one().into_repr();
                                power_of_two.muln(power_of_2);
                                let power_of_two_inv =
                                    P::ScalarField::from(power_of_two).inverse().unwrap();
                                g.mul(power_of_two_inv)
                            })
                        },
                        mode,
                    )?;

                    (ge, BitIterator::new(modulus_minus_1.as_ref()))
                };
                // Remove the even part of the cofactor
                for _ in 0..power_of_2 {
                    ge.double_in_place()?;
                }

                let mut seen_one = false;
                let mut result = Self::zero();
                for b in iter {
                    let old_seen_one = seen_one;
                    if seen_one {
                        result.double_in_place()?;
                    } else {
                        seen_one = b;
                    }

                    if b {
                        result = if old_seen_one {
                            result + &ge
                        } else {
                            ge.clone()
                        };
                    }
                }
                if cofactor_weight < modulus_minus_1_weight {
                    Ok(result)
                } else {
                    ge.enforce_equal(&ge)?;
                    Ok(ge)
                }
            }
        }
    }
}

#[inline]
fn div2(limbs: &mut [u64]) {
    let mut t = 0;
    for i in limbs.iter_mut().rev() {
        let t2 = *i << 63;
        *i >>= 1;
        *i |= t;
        t = t2;
    }
}

impl_bounded_ops!(
    AffineVar<P, F>,
    TEProjective<P>,
    Add,
    add,
    AddAssign,
    add_assign,
    |this: &'a AffineVar<P, F>, other: &'a AffineVar<P, F>| {
        if let Some(cs) = [this, other].cs() {
            let a = P::COEFF_A;
            let d = P::COEFF_D;

            // Compute U = (x1 + y1) * (x2 + y2)
            let u1 = (&this.x * -a) + &this.y;
            let u2 = &other.x + &other.y;

            let u = u1 *  &u2;

            // Compute v0 = x1 * y2
            let v0 = &other.y * &this.x;

            // Compute v1 = x2 * y1
            let v1 = &other.x * &this.y;

            // Compute C = d*v0*v1
            let v2 = &v0 * &v1 * d;

            // Compute x3 = (v0 + v1) / (1 + v2)
            let x3 = F::new_witness(cs.ns("x3"), || {
                let t0 = v0.value()? + &v1.value()?;
                let t1 = P::BaseField::one() + &v2.value()?;
                Ok(t0 * &t1.inverse().ok_or(SynthesisError::DivisionByZero)?)
            }).unwrap();

            let v2_plus_one = &v2 + P::BaseField::one();
            let v0_plus_v1 = &v0 + &v1;
            x3.mul_equals(&v2_plus_one, &v0_plus_v1).unwrap();

            // Compute y3 = (U + a * v0 - v1) / (1 - v2)
            let y3 = F::new_witness(cs.ns("y3"), || {
                let t0 = u.value()? + &(a * &v0.value()?) - &v1.value()?;
                let t1 = P::BaseField::one() - &v2.value()?;
                Ok(t0 * &t1.inverse().ok_or(SynthesisError::DivisionByZero)?)
            }).unwrap();

            let one_minus_v2 = (&v2 - P::BaseField::one()).negate().unwrap();
            let a_v0 = &v0 * a;
            let u_plus_a_v0_minus_v1 = &u + &a_v0 - &v1;

            y3.mul_equals(&one_minus_v2, &u_plus_a_v0_minus_v1).unwrap();

            AffineVar::new(x3, y3)
        } else {
            assert!(this.is_constant() && other.is_constant());
            AffineVar::constant(this.value().unwrap() + &other.value().unwrap())
        }
    },
    |this: &'a AffineVar<P, F>, other: TEProjective<P>| this + AffineVar::constant(other),
    (
        F: FieldVar<P::BaseField>
            + TwoBitLookupGadget<<F as FieldVar<P::BaseField>>::ConstraintF, TableConstant = P::BaseField>
            + ThreeBitCondNegLookupGadget<<F as FieldVar<P::BaseField>>::ConstraintF, TableConstant = P::BaseField>,
        P: TEModelParameters,
    ),
    for <'b> &'b F: FieldOpsBounds<'b, P::BaseField, F>,
);

impl_bounded_ops!(
    AffineVar<P, F>,
    TEProjective<P>,
    Sub,
    sub,
    SubAssign,
    sub_assign,
    |this: &'a AffineVar<P, F>, other: &'a AffineVar<P, F>| this + other.negate().unwrap(),
    |this: &'a AffineVar<P, F>, other: TEProjective<P>| this - AffineVar::constant(other),
    (
        F: FieldVar<P::BaseField>
            + TwoBitLookupGadget<<F as FieldVar<P::BaseField>>::ConstraintF, TableConstant = P::BaseField>
            + ThreeBitCondNegLookupGadget<<F as FieldVar<P::BaseField>>::ConstraintF, TableConstant = P::BaseField>,
        P: TEModelParameters,
    ),
    for <'b> &'b F: FieldOpsBounds<'b, P::BaseField, F>
);

impl<'a, P, F> GroupOpsBounds<'a, TEProjective<P>, AffineVar<P, F>> for AffineVar<P, F>
where
    P: TEModelParameters,
    F: FieldVar<P::BaseField>
        + TwoBitLookupGadget<<F as FieldVar<P::BaseField>>::ConstraintF, TableConstant = P::BaseField>
        + ThreeBitCondNegLookupGadget<
            <F as FieldVar<P::BaseField>>::ConstraintF,
            TableConstant = P::BaseField,
        >,

    for<'b> &'b F: FieldOpsBounds<'b, P::BaseField, F>,
{
}

impl<'a, P, F> GroupOpsBounds<'a, TEProjective<P>, AffineVar<P, F>> for &'a AffineVar<P, F>
where
    P: TEModelParameters,
    F: FieldVar<P::BaseField>
        + TwoBitLookupGadget<<F as FieldVar<P::BaseField>>::ConstraintF, TableConstant = P::BaseField>
        + ThreeBitCondNegLookupGadget<
            <F as FieldVar<P::BaseField>>::ConstraintF,
            TableConstant = P::BaseField,
        >,
    for<'b> &'b F: FieldOpsBounds<'b, P::BaseField, F>,
{
}

impl<P, F> CondSelectGadget<F::ConstraintF> for AffineVar<P, F>
where
    P: TEModelParameters,
    F: FieldVar<P::BaseField>,
    for<'b> &'b F: FieldOpsBounds<'b, P::BaseField, F>,
{
    #[inline]
    fn conditionally_select(
        cond: &Boolean<F::ConstraintF>,
        true_value: &Self,
        false_value: &Self,
    ) -> Result<Self, SynthesisError> {
        let x = cond.select(&true_value.x, &false_value.x)?;
        let y = cond.select(&true_value.y, &false_value.y)?;

        Ok(Self::new(x, y))
    }
}

impl<P, F> EqGadget<F::ConstraintF> for AffineVar<P, F>
where
    P: TEModelParameters,
    F: FieldVar<P::BaseField>,
    for<'b> &'b F: FieldOpsBounds<'b, P::BaseField, F>,
{
    fn is_eq(&self, other: &Self) -> Result<Boolean<F::ConstraintF>, SynthesisError> {
        let x_equal = self.x.is_eq(&other.x)?;
        let y_equal = self.y.is_eq(&other.y)?;
        x_equal.and(&y_equal)
    }

    #[inline]
    fn conditional_enforce_equal(
        &self,
        other: &Self,
        condition: &Boolean<F::ConstraintF>,
    ) -> Result<(), SynthesisError> {
        self.x.conditional_enforce_equal(&other.x, condition)?;
        self.y.conditional_enforce_equal(&other.y, condition)?;
        Ok(())
    }

    #[inline]
    fn conditional_enforce_not_equal(
        &self,
        other: &Self,
        condition: &Boolean<F::ConstraintF>,
    ) -> Result<(), SynthesisError> {
        self.is_eq(other)?
            .and(condition)?
            .enforce_equal(&Boolean::Constant(false))
    }
}

impl<P, F> ToBitsGadget<F::ConstraintF> for AffineVar<P, F>
where
    P: TEModelParameters,
    F: FieldVar<P::BaseField>,
    for<'b> &'b F: FieldOpsBounds<'b, P::BaseField, F>,
{
    fn to_bits(&self) -> Result<Vec<Boolean<F::ConstraintF>>, SynthesisError> {
        let mut x_bits = self.x.to_bits()?;
        let y_bits = self.y.to_bits()?;
        x_bits.extend_from_slice(&y_bits);
        Ok(x_bits)
    }

    fn to_non_unique_bits(&self) -> Result<Vec<Boolean<F::ConstraintF>>, SynthesisError> {
        let mut x_bits = self.x.to_non_unique_bits()?;
        let y_bits = self.y.to_non_unique_bits()?;
        x_bits.extend_from_slice(&y_bits);

        Ok(x_bits)
    }
}

impl<P, F> ToBytesGadget<F::ConstraintF> for AffineVar<P, F>
where
    P: TEModelParameters,
    F: FieldVar<P::BaseField>,
    for<'b> &'b F: FieldOpsBounds<'b, P::BaseField, F>,
{
    fn to_bytes(&self) -> Result<Vec<UInt8<F::ConstraintF>>, SynthesisError> {
        let mut x_bytes = self.x.to_bytes()?;
        let y_bytes = self.y.to_bytes()?;
        x_bytes.extend_from_slice(&y_bytes);
        Ok(x_bytes)
    }

    fn to_non_unique_bytes(&self) -> Result<Vec<UInt8<F::ConstraintF>>, SynthesisError> {
        let mut x_bytes = self.x.to_non_unique_bytes()?;
        let y_bytes = self.y.to_non_unique_bytes()?;
        x_bytes.extend_from_slice(&y_bytes);

        Ok(x_bytes)
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn test<P, GG>() -> Result<(), SynthesisError>
where
    P: TEModelParameters,
    GG: GroupVar<TEProjective<P>>,
    for<'a> &'a GG: GroupOpsBounds<'a, TEProjective<P>, GG>,
{
    use crate::prelude::*;
    use algebra::{test_rng, Group, UniformRand};
    use r1cs_core::ConstraintSystem;

    crate::groups::test::group_test::<TEProjective<P>, GG>()?;

    let mut rng = test_rng();

    let cs = ConstraintSystem::<GG::ConstraintF>::new_ref();

    let a = TEProjective::<P>::rand(&mut rng);
    let b = TEProjective::<P>::rand(&mut rng);
    let a_affine = a.into_affine();
    let b_affine = b.into_affine();

    println!("Allocating things");
    let ns = cs.ns("allocating variables");
    println!("{:?}", cs.current_namespace());
    let mut gadget_a = GG::new_witness(cs.ns("a"), || Ok(a))?;
    let gadget_b = GG::new_witness(cs.ns("b"), || Ok(b))?;
    println!("{:?}", cs.current_namespace());
    ns.leave_namespace();
    println!("Done Allocating things");
    assert_eq!(gadget_a.value()?.into_affine().x, a_affine.x);
    assert_eq!(gadget_a.value()?.into_affine().y, a_affine.y);
    assert_eq!(gadget_b.value()?.into_affine().x, b_affine.x);
    assert_eq!(gadget_b.value()?.into_affine().y, b_affine.y);
    assert_eq!(cs.which_is_unsatisfied(), None);

    println!("Checking addition");
    // Check addition
    let ab = a + &b;
    let ab_affine = ab.into_affine();
    let gadget_ab = &gadget_a + &gadget_b;
    let gadget_ba = &gadget_b + &gadget_a;
    gadget_ba.enforce_equal(&gadget_ab)?;

    let ab_val = gadget_ab.value()?.into_affine();
    assert_eq!(ab_val, ab_affine, "Result of addition is unequal");
    assert!(cs.is_satisfied().unwrap());
    println!("Done checking addition");

    println!("Checking doubling");
    // Check doubling
    let aa = Group::double(&a);
    let aa_affine = aa.into_affine();
    gadget_a.double_in_place()?;
    let aa_val = gadget_a.value()?.into_affine();
    assert_eq!(
        aa_val, aa_affine,
        "Gadget and native values are unequal after double."
    );
    assert!(cs.is_satisfied().unwrap());
    println!("Done checking doubling");

    println!("Checking mul_bits");
    // Check mul_bits
    let scalar = P::ScalarField::rand(&mut rng);
    let native_result = AffineCurve::mul(&aa.into_affine(), scalar);
    let native_result = native_result.into_affine();

    let mut scalar: Vec<bool> = BitIterator::new(scalar.into_repr()).collect();
    // Get the scalar bits into little-endian form.
    scalar.reverse();
    let input: Vec<Boolean<_>> = Vec::new_witness(cs.ns("bits"), || Ok(scalar)).unwrap();
    let result = gadget_a.mul_bits(input.iter())?;
    let result_val = result.value()?.into_affine();
    assert_eq!(
        result_val, native_result,
        "gadget & native values are diff. after scalar mul"
    );
    assert!(cs.is_satisfied().unwrap());
    println!("Done checking mul_bits");

    if !cs.is_satisfied().unwrap() {
        println!("Not satisfied");
        println!("{:?}", cs.which_is_unsatisfied().unwrap());
    }

    assert!(cs.is_satisfied().unwrap());
    Ok(())
}

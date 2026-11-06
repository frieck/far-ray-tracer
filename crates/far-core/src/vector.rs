//! 3D vector and axis-aligned box.
//!
//! Floating-point operations run in the same order as in the C++ original, so
//! images can be compared with it pixel by pixel.

use std::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub};

pub const EPSILON: f32 = 0.0001;
pub const PI: f32 = 3.141_592_653_589_793_238_462;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub type Color = Vec3;

impl Vec3 {
    #[inline(always)]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Dot product.
    #[inline(always)]
    pub fn dot(self, b: Self) -> f32 {
        self.x * b.x + self.y * b.y + self.z * b.z
    }

    #[inline(always)]
    pub fn cross(self, b: Self) -> Self {
        Self::new(
            self.y * b.z - self.z * b.y,
            self.z * b.x - self.x * b.z,
            self.x * b.y - self.y * b.x,
        )
    }

    #[inline(always)]
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// Normalizes in place.
    #[inline(always)]
    pub fn normalize(&mut self) {
        let l = 1.0 / self.length();
        self.x *= l;
        self.y *= l;
        self.z *= l;
    }

    #[inline(always)]
    pub fn array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }

    #[inline(always)]
    pub fn cell(self, i: usize) -> f32 {
        self.array()[i]
    }

    #[inline(always)]
    pub fn set_cell(&mut self, i: usize, v: f32) {
        match i {
            0 => self.x = v,
            1 => self.y = v,
            _ => self.z = v,
        }
    }
}

impl Add for Vec3 {
    type Output = Self;
    #[inline(always)]
    fn add(self, b: Self) -> Self {
        Self::new(self.x + b.x, self.y + b.y, self.z + b.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;
    #[inline(always)]
    fn sub(self, b: Self) -> Self {
        Self::new(self.x - b.x, self.y - b.y, self.z - b.z)
    }
}

impl Neg for Vec3 {
    type Output = Self;
    #[inline(always)]
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;
    #[inline(always)]
    fn mul(self, f: f32) -> Self {
        Self::new(self.x * f, self.y * f, self.z * f)
    }
}

impl Mul<Vec3> for f32 {
    type Output = Vec3;
    #[inline(always)]
    fn mul(self, v: Vec3) -> Vec3 {
        v * self
    }
}

/// Component-wise product, used to blend colors.
impl Mul for Vec3 {
    type Output = Self;
    #[inline(always)]
    fn mul(self, b: Self) -> Self {
        Self::new(self.x * b.x, self.y * b.y, self.z * b.z)
    }
}

impl AddAssign for Vec3 {
    #[inline(always)]
    fn add_assign(&mut self, b: Self) {
        self.x += b.x;
        self.y += b.y;
        self.z += b.z;
    }
}

impl MulAssign<f32> for Vec3 {
    #[inline(always)]
    fn mul_assign(&mut self, f: f32) {
        self.x *= f;
        self.y *= f;
        self.z *= f;
    }
}

/// Axis-aligned box: minimum corner and size.
#[derive(Clone, Copy, Debug, Default)]
pub struct Aabb {
    pub pos: Vec3,
    pub size: Vec3,
}

impl Aabb {
    pub fn new(pos: Vec3, size: Vec3) -> Self {
        Self { pos, size }
    }

    /// Half of the surface area, used by the SAH heuristic.
    #[inline]
    pub fn semi_area(&self) -> f32 {
        let (w, h, d) = (self.size.x, self.size.y, self.size.z);
        w * d + w * h + d * h
    }
}

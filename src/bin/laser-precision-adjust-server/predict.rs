use nalgebra::{
    allocator::Allocator,
    base::{DMatrix, Matrix},
    storage::Owned,
    ComplexField, DefaultAllocator, Dim, DimDiff, DimMin, DimMinimum, DimName, DimSub, Dynamic,
    RealField, Scalar, VecStorage, SVD, U1,
};

pub(crate) fn linear_regression<N, R, C /*, R1, C1*/>(
    xs: Matrix<N, R, C, Owned<N, R, C>>,
    //ys: Matrix<N, R1, C1, Owned<N, R1, C1>>,
)
/*-> Matrix<N, R, C, Owned<N, R, C>>*/
where
    R: DimMin<C>,
    C: Dim,
    N: ComplexField,
    DimMinimum<R, C>: DimSub<U1>,
    DefaultAllocator: Allocator<N, R, C>
        + Allocator<N, C>
        + Allocator<N, R>
        + Allocator<N, DimDiff<DimMinimum<R, C>, U1>>
        + Allocator<N, DimMinimum<R, C>, C>
        + Allocator<N, R, DimMinimum<R, C>>
        + Allocator<N, DimMinimum<R, C>>
        + Allocator<N::RealField, DimMinimum<R, C>>
        + Allocator<N::RealField, DimDiff<DimMinimum<R, C>, U1>>,
    /*
    R1: DimMin<C1>,
    C1: Dim,
    R1: DimName,
    C1: DimName,
    DimMinimum<R1, C1>: DimSub<U1>,
    DefaultAllocator: Allocator<N, R1, C1>
        + Allocator<N, C1>
        + Allocator<N, R1>
        + Allocator<N, DimDiff<DimMinimum<R1, C1>, U1>>
        + Allocator<N, DimMinimum<R1, C1>, C1>
        + Allocator<N, R1, DimMinimum<R1, C1>>
        + Allocator<N, DimMinimum<R1, C1>>
        + Allocator<N::RealField, DimMinimum<R1, C1>>
        + Allocator<N::RealField, DimDiff<DimMinimum<R1, C1>, U1>>,
        */
{
    let svd = SVD::new(xs, true, true);
    /*
    let order = xs.cols() - 1;

    let u = svd.get_u();
    // cut down s matrix to the expected number of rows given order
    let s_hat = svd.get_s().filter_rows(&|_, row| row <= order);
    let v = svd.get_v();

    let alpha = u.t() * ys;
    let mut mdata = vec![];
    for i in 0..(order + 1) {
        mdata.push(alpha.get(i, 0) / s_hat.get(i, i));
    }
    let sinv_alpha = Matrix::new(order + 1, 1, mdata);

    v * sinv_alpha
    */
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn matrix_create_access() {
        let data = DMatrix::from_vec(2, 2, vec![0, 0, 1, 3]);

        assert_eq!(data[(0, 0)], 0);
        assert_eq!(data[(1, 1)], 3)
    }

    #[test]
    fn svd_decomposition() {
        let data = DMatrix::from_vec(
            7,
            2,
            vec![0., 0., 1., 3., 2., 7., 3., 12., 4., 22., 5., 31., 6., 43.],
        );

        let ys = data.column(0);
        let xs = DMatrix::from_fn(ys.nrows(), 3, |row, col| row.pow(col as u32) as f64).to_owned();
        let betas = linear_regression(xs /*, ys*/);
    }
}

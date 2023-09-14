
# -*- codding: utf-8 -*-

import math

__author__ = 'tolyan'


class Point:
    ''' Точка с координатами X, Y '''

    def __init__(self, X=0, Y=0):
        self.X = X
        self.Y = Y


class EmptySplineFragment:
    ''' Пустой фрагмет, всегда NAN '''

    X_LESS = -1
    X_IN = 0
    X_OVER = 1

    def Y(self, X):
        ''' Значение в точке '''
        return math.nan

    def dY(self, X):
        ''' Значение производной в точке '''
        return math.nan

    def d2Y(self, X):
        ''' Значение второй производной в точке '''
        return math.nan

    def isXin(self, X):
        ''' Проверка, входит ли X в этот фрагмент '''
        return EmptySplineFragment.X_IN


class SplineFragment(EmptySplineFragment):
    ''' Фрагмент сплайна третьего порядка '''

    def __init__(self, A=0, B=0, C=0, D=0, Xmin=0, Xmax=0):
        self.A = A
        self.B = B
        self.C = C
        self.D = D
        self.Xmin = Xmin
        self.Xmax = Xmax

    def Y(self, X):
        ''' Значение в точке '''
        x = X - self.Xmin
        x2 = x * x
        x3 = x2 * x
        return self.A * x3 + self.B * x2 + self.C * x + self.D

    def dY(self, X):
        ''' Значение производной в точке '''
        x = X - self.Xmin
        x2 = x * x
        return 3 * self.A * x2 + 2 * self.B * 2 + self.C

    def d2Y(self, X):
        ''' Значение второй производной в точке '''
        x = X - self.Xmin
        return 6 * self.A * x + 2 * self.B

    def isXin(self, X):
        ''' Проверка, входит ли X в этот фрагмент '''
        if (X < self.Xmin):
            return EmptySplineFragment.X_LESS
        elif (X > self.Xmax):
            return EmptySplineFragment.X_OVER
        else:
            return EmptySplineFragment.X_IN


class SmoothSpline:
    ''' сглаживающий сплайн '''

    def __init__(self):
        self.Points = [Point()]
        self._spline_fragments = [EmptySplineFragment()]

    def Calc_old(self, A, B, C, D, E, F, N1, N2, N3, pg):
        for i in range(1, N2 + 1):
            ii = i - 1
            i1 = i + 1
            i2 = i + 2
            hii = self.Points[i].X - self.Points[ii].X
            hi = self.Points[i1].X - self.Points[i].X
            hi1 = self.Points[i2].X - self.Points[i1].X
            D[i] = hi / 6.0 - 1.0 / hi * ((1.0 / hii + 1.0 / hi) \
                                          * pg + (1.0 / hi + 1.0 / hi1) * pg)
            B[i1] = D[i]
        for i in range(1, N3 + 1):
            i1 = i + 1
            i2 = i + 2
            hi = self.Points[i1].X - self.Points[i].X
            hi1 = self.Points[i2].X - self.Points[i1].X
            E[i] = pg / (hi * hi1)
            A[i2] = E[i]
        for i in range(1, N1 + 1):
            ii = i - 1
            i1 = i + 1
            hii = self.Points[i].X - self.Points[ii].X
            hi = self.Points[i1].X - self.Points[i].X
            F[i] = (self.Points[i1].Y - self.Points[i].Y) / hi - (self.Points[i].Y - self.Points[ii].Y) / hii
            C[i] = (hii + hi) / 3 + 1 / (hii * hii) * pg + (1 / hii + 1 / hi) * \
                                                           (1 / hii + 1 / hi) * pg + pg / (hi * hi)

    def _calc_new(self, A, B, C, D, E, F, N1, N2, N3, pg):
        for i in range(1, N1 + 1):
            ii = i - 1
            i1 = i + 1
            hii = self.Points[i].X - self.Points[ii].X
            hi = self.Points[i1].X - self.Points[i].X
            F[i] = (self.Points[i1].Y - self.Points[i].Y) / hi - (self.Points[i].Y - self.Points[ii].Y) / hii
            C[i] = (hii + hi) / 3 + 1 / (hii * hii) * pg + (1 / hii + 1 / hi) * \
                                                           (1 / hii + 1 / hi) * pg + pg / (hi * hi)
            if i < N2 + 1:
                i2 = i + 2
                hi1 = self.Points[i2].X - self.Points[i1].X
                D[i] = hi / 6.0 - 1.0 / hi * ((1.0 / hii + 1.0 / hi) \
                                          * pg + (1.0 / hi + 1.0 / hi1) * pg)
                B[i1] = D[i]
                if i < N3 + 1:
                    E[i] = pg / (hi * hi1)
                    A[i2] = E[i]


    def _prepareCalc(self, pg):
        NWin = len(self.Points)

        lastN = NWin - 1

        N1 = lastN - 1
        N2 = N1 - 1
        N3 = N2 - 1

        A = [0.0] * NWin
        B = [0.0] * NWin
        C = [0.0] * NWin
        D = [0.0] * NWin
        E = [0.0] * NWin
        F = [0.0] * NWin
        P = [0.0] * NWin
        Q = [0.0] * NWin
        CM = [0.0] * NWin
        ym = [0.0] * NWin

        y1 = (self.Points[1].Y - self.Points[0].Y) / (self.Points[1].X - self.Points[0].X)
        h1 = self.Points[1].X - self.Points[0].X
        h2 = self.Points[2].X - self.Points[1].X

        C[0] = h1 / 3.0 + 2.0 / (h1 * h1) * pg
        D[0] = h1 / 6.0 - 1.0 / h1 * (1.0 / h1 + 1.0 / h2) * pg - pg / (h1 * h1)
        E[0] = pg / (h1 * h2)
        F[0] = (self.Points[1].Y - self.Points[0].Y) / h1 - y1
        B[1] = D[0]
        A[2] = E[0]

        yn = (self.Points[lastN].Y - self.Points[N1].Y) / (self.Points[lastN].X - self.Points[N1].X)
        hn1 = self.Points[lastN].X - self.Points[N1].X
        hn2 = self.Points[N1].X - self.Points[N2].X

        A[lastN] = pg / (hn1 * hn2)
        B[lastN] = hn1 / 6.0 - 1.0 / hn1 * (1.0 / hn1 + 1.0 / hn2) * pg - pg / (hn1 * hn1)
        C[lastN] = hn1 / 3.0 + 2.0 / (hn1 * hn1) * pg
        D[N1] = B[lastN]
        E[N2] = A[lastN]
        F[lastN] = yn - (self.Points[lastN].Y - self.Points[N1].Y) / hn1

        return  (A, B, C, D, E, F, P, Q, CM, ym, lastN, N1, N2, N3)

    def _solve(self, A, B, C, D, E, F, P, Q, CM, ym, N1, lastN, pg):
        P[1] = -D[1] / C[1]
        Q[1] = -E[1] / C[1]
        CM[1] = F[1] / C[1]
        CP = C[2] + B[2] * P[1]
        P[2] = -(D[2] + B[2] * Q[1]) / CP
        Q[2] = -E[2] / CP
        CM[2] = (F[2] - B[2] * CM[1]) / CP
        E[N1] = 0
        D[lastN] = 0
        E[lastN] = 0

        for i in range(2, lastN):
            i1 = i - 1
            i2 = i - 2
            CB = A[i] * P[i2] + B[i]
            CK = C[i] + CB * P[i1] + A[i] * Q[i2]
            P[i] = -(D[i] + CB * Q[i1]) / CK
            Q[i] = -E[i] / CK
            CM[i] = (F[i] - CB * CM[i1] - A[i] * CM[i2]) / CK

        CM[N1] = P[N1] * CM[lastN] + CM[N1]

        for i in range(2, lastN):
            k = lastN - i
            k1 = k + 1
            k2 = k + 2
            CM[k] = P[k] * CM[k1] + Q[k] * CM[k2] + CM[k]

        for i in range(1, N1 + 1):
            ii = i - 1
            i1 = i + 1
            hii = self.Points[i].X - self.Points[ii].X
            hi = self.Points[i1].X - self.Points[i].X
            ym[i] = self.Points[i].Y - pg * ((CM[i1] - CM[i]) / hi - (CM[i] - CM[ii]) / hii)

        ym[0] = self.Points[0].Y - pg*(CM[1] - CM[0]) / (self.Points[1].X - self.Points[0].X)
        ym[lastN] = self.Points[lastN].Y + pg*(CM[lastN] - CM[N1]) / self.Points[lastN].X - self.Points[N1].X

        return (A, B, C, D, E, F, P, Q, CM, ym)

    def _createFragments(self, CM, ym, lastN):
        self._spline_fragments = [ SplineFragment() for i in range(len(CM)) ]

        self._spline_fragments[0].Xmin = self.Points[0].X

        for i in range(1, lastN):
            res_index = i - 1

            hi = self.Points[i].X - self.Points[i - 1].X
            fragment = self._spline_fragments[res_index]

            fragment.Xmax = self.Points[i].X
            if res_index > 0:
                fragment.Xmin = self._spline_fragments[res_index - 1].Xmax

            fragment.A = (CM[i] - CM[i-1]) / (6 * hi)
            fragment.B = CM[i - 1] / 2
            fragment.C = (ym[i] - ym[i-1]) / hi - (2 * CM[i - 1] + CM[i]) * hi / 6
            fragment.D =ym[i - 1]

    def Update(self, pg):
        ''' Пересчитать сплайн по новым данным self.Points '''
        NWin = len(self.Points)
        if NWin == 0:
            self._spline_fragments = [EmptySplineFragment()]

        A, B, C, D, E, F, P, Q, CM, ym, lastN, N1, N2, N3 = self._prepareCalc(pg)
        self._calc_new(A, B, C, D, E, F, N1, N2, N3, pg)
        self._solve(A, B, C, D, E, F, P, Q, CM, ym, N1, lastN, pg)
        self._createFragments(CM, ym, lastN)

    def _find_fragment(self, X):
        start_element = 0
        end_element = len(self._spline_fragments) - 1
        middle = int(end_element / 2)
        while start_element < end_element:
            fragment = self._spline_fragments[middle]
            res = fragment.isXin(X)
            if res == SplineFragment.X_IN:
                return fragment
            elif res == SplineFragment.X_OVER:
                start_element = middle + 1
            else:
                end_element = middle - 1
            middle = int((start_element + end_element) / 2)

        return self._spline_fragments[middle] if \
            self._spline_fragments[middle].isXin(X) == SplineFragment.X_IN else EmptySplineFragment()


    def Y(self, X):
        ''' Значение сплайна в точке '''
        return self._find_fragment(X).Y(X)

    def dY(self, X):
        ''' Значение производной сплайна в точке '''
        return self._find_fragment(X).dY(X)

    def d2Y(self, X):
        ''' Значение второй производной сплайна в точке '''
        return self._find_fragment(X).d2Y(X)


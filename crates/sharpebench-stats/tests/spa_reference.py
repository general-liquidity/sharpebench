"""Independent rational oracle for spa_studentization.rs, not a coverage proof.

Run with Python's standard library. Means, variances and squared positive
statistics are rational; only consistent-SPA exclusion uses log/sqrt floats.
"""
from fractions import Fraction as F
from math import log, sqrt

MASK = (1 << 64) - 1
FIELD = [
    [F(x) for x in column]
    for column in [
        [.28125, -.5, .75, -.25, .5, -.75, .25, 0,
         .5, -.25, .75, -.5, .25, 0, -.25, .5],
        [4, -8, 2, -4, 8, -2, 0, 4, -8, 2, -4, 8, -2, 0, 4, -2],
        [-.5, -1.5, 0, -2, -.5, -1, -1.5, 0,
         -2, -.5, -1, -1.5, 0, -2, -.5, -1],
    ]
]


class RNG:
    def __init__(self, seed):
        self.state = seed ^ 0x59A050A02026BEEF

    def unit(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return ((z ^ (z >> 31)) >> 11) / 2**53

    def below(self):
        return int(self.unit() * 16)


def counts(seed=7, draws=127, scale=F(1), wrong_center=False):
    field = [column[:] for column in FIELD]
    field[0] = [x * scale for x in field[0]]
    means = [sum(column, F(0)) / 16 for column in field]
    rng = RNG(seed)
    rows = []
    for _ in range(draws):
        index = rng.below()
        indices = []
        for _ in range(16):
            indices.append(index)
            # The last observation also consumes a restart decision.
            index = rng.below() if rng.unit() < .5 else (index + 1) % 16
        rows.append([
            4 * (sum((column[i] for i in indices), F(0)) / 16 - mean)
            for column, mean in zip(field, means)
        ])
    scales_squared = []
    for k in range(3):
        column = [row[k] for row in rows]
        center = sum(column, F(0)) / draws
        if wrong_center:
            center = -center
        variance = sum(((x - center) ** 2 for x in column), F(0)) / draws
        scales_squared.append(max(variance, F.from_float(1e-8) ** 2))
    observed = max(
        16 * mean**2 / variance if mean > 0 else F(0)
        for mean, variance in zip(means, scales_squared)
    )
    excluded = [
        4 * float(mean) / sqrt(float(variance)) < -sqrt(2 * log(log(16)))
        for mean, variance in zip(means, scales_squared)
    ]
    result = []
    for consistent in [False, True]:
        boot = [max(
            x**2 / variance if x > 0 and not (consistent and bad) else F(0)
            for x, variance, bad in zip(row, scales_squared, excluded)
        ) for row in rows]
        result.append((
            sum(value >= observed for value in boot),
            sum(value == observed for value in boot),
            min(float(abs(value - observed)) for value in boot),
        ))
    return result


if __name__ == "__main__":
    for scale, expected in [(F(1), [34, 26]), (F(1, 2**27), [74, 57])]:
        result = counts(scale=scale)
        assert [row[0] for row in result] == expected, result
        assert [row[1] for row in result] == [0, 0], result
        print(f"seed=7 draws=127 scale={scale}: {result}")
    result = counts(seed=2, draws=7)
    assert [row[0] for row in result] == [6, 5], result
    assert [row[1] for row in result] == [0, 0], result
    print(f"seed=2 draws=7 scale=1: {result}")

<?php

namespace App;

function helper(int $x): int
{
    return $x + 1;
}

class Calculator
{
    public function add(int $a, int $b): int
    {
        return $a + $b;
    }

    public function compute(): int
    {
        // self:: → App\Calculator::add (resolved, in-workspace).
        $r = self::add(1, 2);
        // C::method → App\Calculator::add (resolved, in-workspace).
        $r = Calculator::add($r, 3);
        // free function → App\helper (resolved, in-workspace).
        $r = helper($r);
        // $this->method → App\Calculator::add (resolved via receiver resolution, cfdb-062-C).
        $this->add($r, 4);
        // external/unknown free function → no CALLS (closed-world).
        $r = missing($r);
        return $r;
    }
}

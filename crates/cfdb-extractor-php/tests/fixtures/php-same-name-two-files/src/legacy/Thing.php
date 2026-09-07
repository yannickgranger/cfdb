<?php

namespace App;

trait Thing
{
    public function legacyOnly(): string
    {
        return 'legacy';
    }
}

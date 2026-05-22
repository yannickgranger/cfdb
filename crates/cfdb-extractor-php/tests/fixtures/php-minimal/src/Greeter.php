<?php

namespace App;

interface Greeter
{
    public function greet(User $u): string;
}

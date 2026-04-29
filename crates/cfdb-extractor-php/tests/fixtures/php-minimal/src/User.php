<?php

namespace App;

class User
{
    private string $displayName;

    public function __construct(string $displayName)
    {
        $this->displayName = $displayName;
    }

    public function name(): string
    {
        return $this->displayName;
    }
}

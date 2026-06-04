<?php

namespace Shop;

interface Identifiable
{
    public function id(): int;
}

interface Serializable
{
    public function toArray(): array;
}

interface Timestamped
{
    public function createdAt(): int;
}

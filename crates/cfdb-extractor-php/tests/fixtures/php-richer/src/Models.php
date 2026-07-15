<?php

namespace Shop;

class Product implements Identifiable, Serializable
{
    public function id(): int
    {
        return 0;
    }

    public function toArray(): array
    {
        return [];
    }
}

// `extends Product` is a base_clause (inheritance) — NOT an IMPLEMENTS edge
// (RFC-045 §3.3 D3-a defers inheritance). Only `implements Timestamped`
// produces an edge here.
class Order extends Product implements Timestamped
{
    public function createdAt(): int
    {
        return 0;
    }
}

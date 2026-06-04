import { Identifiable, Serializable, Timestamped } from "./contracts";

export class Product implements Identifiable, Serializable {
    id(): number {
        return 0;
    }
    toJSON(): string {
        return "";
    }
}

// `extends Product` is an extends_clause (inheritance) — NOT an IMPLEMENTS
// edge (RFC-045 §3.3 D3-a). Only `implements Timestamped` produces an edge.
export class Order extends Product implements Timestamped {
    createdAt(): number {
        return 0;
    }
}

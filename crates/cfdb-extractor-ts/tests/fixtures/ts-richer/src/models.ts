import { Identifiable, Serializable, Timestamped } from "./contracts";

export class Product implements Identifiable, Serializable {
    id(): number {
        return 0;
    }
    toJSON(): string {
        // A call site so the fixture exercises 45-D :CallSite emission:
        // `this.id()` (member_expression callee_path) inside Product::toJSON.
        const n = this.id();
        return String(n);
    }
}

// `extends Product` is an extends_clause (inheritance) — NOT an IMPLEMENTS
// edge (RFC-045 §3.3 D3-a). Only `implements Timestamped` produces an edge.
export class Order extends Product implements Timestamped {
    createdAt(): number {
        return 0;
    }
}

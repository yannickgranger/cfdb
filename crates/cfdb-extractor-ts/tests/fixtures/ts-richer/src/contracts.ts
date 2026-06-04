export interface Identifiable {
    id(): number;
}

export interface Serializable {
    toJSON(): string;
}

export interface Timestamped {
    createdAt(): number;
}

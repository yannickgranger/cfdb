export interface User {
    name: string;
    age: number;
}

export type UserId = string;

export function makeUser(name: string, age: number): User {
    return { name, age };
}

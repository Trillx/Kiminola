import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";
import type { Snippet } from "svelte";

export function cn(...inputs: ClassValue[]) {
	return twMerge(clsx(inputs));
}

export type WithoutChildren<T> = Omit<T, "children">;

export type WithoutChild<T> = T & {
	children?: Snippet;
};

export type WithoutChildrenOrChild<T> = WithoutChildren<WithoutChild<T>>;

export type WithElementRef<T> = T & {
	ref?: HTMLElement | null;
};

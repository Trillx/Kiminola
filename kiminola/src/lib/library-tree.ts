import type { LibraryLocation, LibraryNode } from "$lib/tauri";

export function recordingHrefForLocation(location: LibraryLocation | null): string {
  if (!location) return "/record";
  const params = new URLSearchParams();
  if (location.kind === "space") params.set("spaceId", String(location.id));
  else params.set("parentMeetingId", String(location.id));
  return `/record?${params.toString()}`;
}

export function locationFromSearchParams(params: URLSearchParams): LibraryLocation | null {
  const spaceId = Number(params.get("spaceId"));
  const parentMeetingId = Number(params.get("parentMeetingId"));
  if (Number.isInteger(parentMeetingId) && parentMeetingId > 0) {
    return { kind: "meeting", id: parentMeetingId };
  }
  if (Number.isInteger(spaceId) && spaceId > 0) {
    return { kind: "space", id: spaceId };
  }
  return null;
}

export function recordingLocationFromSearchParams(
  params: URLSearchParams,
  lastLocation: LibraryLocation | null,
  recoveryLocation: LibraryLocation | null = null,
): LibraryLocation | null {
  return locationFromSearchParams(params) ?? recoveryLocation ?? lastLocation;
}

export function nodeRef(node: LibraryNode): LibraryLocation {
  return { kind: node.kind, id: node.id };
}

export function nodeKey(ref: LibraryLocation): string {
  return `${ref.kind}:${ref.id}`;
}

export function destinationKey(location: LibraryLocation | null): string {
  return location ? nodeKey(location) : "library-root";
}

function findNode(nodes: LibraryNode[], ref: LibraryLocation): LibraryNode | null {
  for (const node of nodes) {
    if (node.kind === ref.kind && node.id === ref.id) return node;
    const found = findNode(node.children, ref);
    if (found) return found;
  }
  return null;
}

export interface LibraryMoveValidation {
  canDrop: (target: LibraryLocation | null) => boolean;
}

// Build once per source/tree snapshot, then share O(1) checks across the move
// dialog and every drag target. Keys include kind because IDs can overlap.
export function createMoveValidation(
  tree: LibraryNode[],
  source: LibraryLocation | null,
): LibraryMoveValidation {
  const sourceNode = source ? findNode(tree, source) : null;
  const descendants = new Set<string>();
  const pending = sourceNode ? [sourceNode] : [];
  while (pending.length) {
    const node = pending.pop()!;
    descendants.add(nodeKey(node));
    pending.push(...node.children);
  }
  const atRoot = source && tree.some((node) => nodeKey(node) === nodeKey(source));
  return {
    canDrop(target) {
      if (!source || !sourceNode) return false;
      if (!target) return source.kind === "space" && !atRoot;
      if (source.kind === "space" && target.kind !== "space") return false;
      return !descendants.has(nodeKey(target));
    },
  };
}

export function canDropNode(
  source: LibraryLocation,
  target: LibraryLocation,
  tree: LibraryNode[],
): boolean {
  return createMoveValidation(tree, source).canDrop(target);
}

export interface LibraryDestinationOption {
  location: LibraryLocation | null;
  label: string;
  depth: number;
  disabled: boolean;
}

export function moveOptions(
  tree: LibraryNode[],
  source: LibraryLocation | null,
  validation = createMoveValidation(tree, source),
): LibraryDestinationOption[] {
  if (!source) return [];
  const sourceLocation = source;
  const options: LibraryDestinationOption[] = [];
  if (sourceLocation.kind === "space") {
    options.push({
      location: null,
      label: "Library root",
      depth: 0,
      disabled: !validation.canDrop(null),
    });
  }
  function visit(nodes: LibraryNode[], depth: number, path: string[]) {
    for (const node of nodes) {
      const location = nodeRef(node);
      const name = node.kind === "space" ? node.name : node.title;
      const nextPath = [...path, name];
      options.push({
        location,
        label: nextPath.join(" / "),
        depth,
        disabled: !validation.canDrop(location),
      });
      visit(node.children, depth + 1, nextPath);
    }
  }
  visit(tree, 0, []);
  return options;
}

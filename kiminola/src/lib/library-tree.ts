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
): LibraryLocation | null {
  return locationFromSearchParams(params) ?? lastLocation;
}

export function nodeRef(node: LibraryNode): LibraryLocation {
  return { kind: node.kind, id: node.id };
}

export function nodeKey(ref: LibraryLocation): string {
  return `${ref.kind}:${ref.id}`;
}

function containsNode(node: LibraryNode, target: LibraryLocation): boolean {
  if (node.kind === target.kind && node.id === target.id) return true;
  return node.children.some((child) => containsNode(child, target));
}

function findNode(nodes: LibraryNode[], ref: LibraryLocation): LibraryNode | null {
  for (const node of nodes) {
    if (node.kind === ref.kind && node.id === ref.id) return node;
    const found = findNode(node.children, ref);
    if (found) return found;
  }
  return null;
}

export function canDropNode(
  source: LibraryLocation,
  target: LibraryLocation,
  tree: LibraryNode[],
): boolean {
  if (source.kind === "space" && target.kind !== "space") return false;
  if (source.kind === target.kind && source.id === target.id) return false;
  const sourceNode = findNode(tree, source);
  return !sourceNode || !containsNode(sourceNode, target);
}

export interface LibraryDestinationOption {
  location: LibraryLocation;
  label: string;
  depth: number;
  disabled: boolean;
}

export function moveOptions(
  tree: LibraryNode[],
  source: LibraryLocation | null,
): LibraryDestinationOption[] {
  if (!source) return [];
  const sourceLocation = source;
  const options: LibraryDestinationOption[] = [];
  function visit(nodes: LibraryNode[], depth: number, path: string[]) {
    for (const node of nodes) {
      const location = nodeRef(node);
      const name = node.kind === "space" ? node.name : node.title;
      const nextPath = [...path, name];
      const accepts = sourceLocation.kind === "meeting" || node.kind === "space";
      options.push({
        location,
        label: nextPath.join(" / "),
        depth,
        disabled: !accepts || !canDropNode(sourceLocation, location, tree),
      });
      visit(node.children, depth + 1, nextPath);
    }
  }
  visit(tree, 0, []);
  return options;
}

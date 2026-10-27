/** Microsoft's Fluent Emoji (MIT): a static 3D render for previews and the
 * animated PNG (2-3 MB, Git LFS) that becomes the agent's picture on save. */
const STATIC_BASE = "https://raw.githubusercontent.com/microsoft/fluentui-emoji/main/assets";
const ANIMATED_BASE = "https://media.githubusercontent.com/media/microsoft/fluentui-emoji-animated/main/assets";

export type FluentEmoji = { folder: string; emoji: string; label: string; toned?: boolean };

// A curated row for the agent editor: every entry was checked to exist upstream.
export const FLUENT_EMOJIS: FluentEmoji[] = [
  { folder: "Robot", emoji: "🤖", label: "robot" },
  { folder: "Alien", emoji: "👽", label: "alien" },
  { folder: "Brain", emoji: "🧠", label: "brain" },
  { folder: "Nerd face", emoji: "🤓", label: "nerd" },
  { folder: "Thinking face", emoji: "🤔", label: "thinking" },
  { folder: "Star-struck", emoji: "🤩", label: "star-struck" },
  { folder: "Partying face", emoji: "🥳", label: "party" },
  { folder: "Eyes", emoji: "👀", label: "eyes" },
  { folder: "Ghost", emoji: "👻", label: "ghost" },
  { folder: "Skull", emoji: "💀", label: "skull" },
  { folder: "Detective", emoji: "🕵️", label: "detective", toned: true },
  { folder: "Astronaut", emoji: "🧑‍🚀", label: "astronaut", toned: true },
  { folder: "Ninja", emoji: "🥷", label: "ninja", toned: true },
  { folder: "Scientist", emoji: "🧑‍🔬", label: "scientist", toned: true },
  { folder: "Mechanical arm", emoji: "🦾", label: "mechanical arm" },
  { folder: "Rocket", emoji: "🚀", label: "rocket" },
  { folder: "Fire", emoji: "🔥", label: "fire" },
  { folder: "High voltage", emoji: "⚡", label: "zap" },
  { folder: "Crystal ball", emoji: "🔮", label: "crystal ball" },
  { folder: "Owl", emoji: "🦉", label: "owl" },
  { folder: "Fox", emoji: "🦊", label: "fox" },
  { folder: "Wolf", emoji: "🐺", label: "wolf" },
  { folder: "Cat face", emoji: "🐱", label: "cat" },
  { folder: "Dog face", emoji: "🐶", label: "dog" },
  { folder: "Unicorn", emoji: "🦄", label: "unicorn" },
  { folder: "Dragon", emoji: "🐉", label: "dragon" },
  { folder: "Octopus", emoji: "🐙", label: "octopus" },
  { folder: "Shark", emoji: "🦈", label: "shark" },
  { folder: "T-rex", emoji: "🦖", label: "t-rex" },
  { folder: "Hatching chick", emoji: "🐣", label: "chick" },
];

const fileStem = (e: FluentEmoji) => e.folder.toLowerCase().replace(/ /g, "_");
const folder = (e: FluentEmoji) => encodeURIComponent(e.folder) + (e.toned ? "/Default" : "");

export function fluentStaticUrl(e: FluentEmoji): string {
  return `${STATIC_BASE}/${folder(e)}/3D/${fileStem(e)}_3d${e.toned ? "_default" : ""}.png`;
}

export function fluentAnimatedUrl(e: FluentEmoji): string {
  return `${ANIMATED_BASE}/${folder(e)}/animated/${fileStem(e)}_animated${e.toned ? "_default" : ""}.png`;
}

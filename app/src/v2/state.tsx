import {
  createContext,
  useContext,
  type Dispatch,
  type SetStateAction,
} from "react";
import type { PreviewVersion, ThemeId } from "./model";
import type { LocalLauncher } from "../hooks/useLocalLauncher";
export type Preferences = {
  theme: ThemeId;
  mode: "dark" | "light";
  wallpaper: boolean;
  motion: boolean;
  intro: boolean;
  eco: boolean;
  randomOnStartup: boolean;
  launchEffect: "fireflies" | "voxel" | "light";
};
export type PreviewState = {
  runtime?: LocalLauncher;
  prefs: Preferences;
  setPrefs: Dispatch<SetStateAction<Preferences>>;
  navigate: (path: string) => void;
  goBack: (fallback: string) => void;
  versions: PreviewVersion[];
  currentId: string;
  setCurrentId: (id: string) => void;
  scenario: string;
  setScenario: (name: string) => void;
  openAccount: () => void;
  openTasks: () => void;
  openDiagnostics: () => void;
  notify: (message: string) => void;
  launchIndex: number;
  setLaunchIndex: (index: number) => void;
  launchFailure: boolean;
  setLaunchFailure: (failure: boolean) => void;
};
export const PreviewContext = createContext<PreviewState | null>(null);
export function usePreview() {
  const value = useContext(PreviewContext);
  if (!value) throw new Error("Preview provider missing");
  return value;
}

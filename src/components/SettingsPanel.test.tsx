// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CustomSkinMetadata, WidgetPreferences } from "../types";
import { SettingsPanel } from "./SettingsPanel";

const basePreferences: WidgetPreferences = {
  locked: false,
  alwaysOnTop: true,
  widgetMode: "expanded",
  widgetSize: "medium",
  compactSize: 72,
  expandedSize: 306,
  toggleCorner: "ne",
  pinnedProvider: null,
  autoRotateSeconds: 12,
  autoCheckUpdates: true,
  language: "en",
  appearance: "system",
  selectedSkin: "default",
  glassStyle: "dock",
  customSkins: [],
};

const customSkin: CustomSkinMetadata = {
  id: "custom-123-lake",
  name: "Lake",
  fileName: "custom-123-lake.png",
  detectedTone: "light",
  textTone: "auto",
  accentColor: "#123456",
};

const bridge = vi.hoisted(() => ({
  preferences: {} as WidgetPreferences,
  preferencesHandler: null as null | ((value: WidgetPreferences) => void),
  launchHandler: null as null | ((value: boolean) => void),
  isTauri: vi.fn(() => false),
  getPreferences: vi.fn(),
  updatePreferences: vi.fn(),
  getLaunchAtLogin: vi.fn(),
  getAppVersion: vi.fn(),
  setLaunchAtLogin: vi.fn(),
  setAlwaysOnTop: vi.fn(),
  setClickThrough: vi.fn(),
  setWidgetDimensions: vi.fn(),
  setWidgetSize: vi.fn(),
  selectSkin: vi.fn(),
  importCustomSkin: vi.fn(),
  getCustomSkinAsset: vi.fn(),
  updateCustomSkin: vi.fn(),
  deleteCustomSkin: vi.fn(),
  getPlatformCapabilities: vi.fn(),
  listenDesktopEvents: vi.fn(),
}));

vi.mock("../lib/bridge", () => bridge);

beforeEach(() => {
  bridge.preferences = structuredClone(basePreferences);
  bridge.preferencesHandler = null;
  bridge.launchHandler = null;
  bridge.getPreferences.mockImplementation(async () => structuredClone(bridge.preferences));
  bridge.updatePreferences.mockImplementation(async (value: WidgetPreferences) => { bridge.preferences = structuredClone(value); });
  bridge.getLaunchAtLogin.mockResolvedValue(false);
  bridge.getAppVersion.mockResolvedValue("1.0.0");
  bridge.getPlatformCapabilities.mockResolvedValue({ nativeGlass: true, supportsLiquidGlass: false });
  bridge.setLaunchAtLogin.mockImplementation(async (enabled: boolean) => enabled);
  bridge.setAlwaysOnTop.mockImplementation(async (value: boolean) => ({ ...bridge.preferences, alwaysOnTop: value }));
  bridge.setClickThrough.mockImplementation(async (value: boolean) => ({ ...bridge.preferences, locked: value }));
  bridge.setWidgetDimensions.mockImplementation(async (compactSize: number, expandedSize: number) => ({ ...bridge.preferences, compactSize, expandedSize, widgetSize: "custom" }));
  bridge.setWidgetSize.mockImplementation(async (value: WidgetPreferences["widgetSize"]) => ({ ...bridge.preferences, widgetSize: value }));
  bridge.selectSkin.mockImplementation(async (id: string) => ({ ...bridge.preferences, selectedSkin: id }));
  bridge.importCustomSkin.mockResolvedValue(customSkin);
  bridge.getCustomSkinAsset.mockResolvedValue({ id: customSkin.id, dataUrl: "data:image/png;base64,LAKE" });
  bridge.updateCustomSkin.mockImplementation(async (id: string, name: string, textTone: string, accentColor: string) => ({
    ...bridge.preferences,
    customSkins: bridge.preferences.customSkins.map((skin) => skin.id === id ? { ...skin, name, textTone, accentColor } : skin),
  }));
  bridge.deleteCustomSkin.mockImplementation(async () => ({ ...bridge.preferences, selectedSkin: "default", customSkins: [] }));
  bridge.listenDesktopEvents.mockImplementation(async (handlers: { onPreferences: (value: WidgetPreferences) => void; onLaunchAtLogin?: (value: boolean) => void }) => {
    bridge.preferencesHandler = handlers.onPreferences;
    bridge.launchHandler = handlers.onLaunchAtLogin ?? null;
    return () => undefined;
  });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

async function renderSettings(preferences: WidgetPreferences = basePreferences) {
  bridge.preferences = structuredClone(preferences);
  render(<SettingsPanel />);
  await screen.findByRole("heading", { name: "General" });
  await waitFor(() => expect(screen.getByTestId("settings-panel")).toHaveAttribute("aria-busy", "false"));
}

describe("SettingsPanel live controls", () => {
  it("keeps the three sidebar destinations in keyboard focus order and exposes every settings group", async () => {
    await renderSettings({ ...basePreferences, selectedSkin: "glass" });

    const navigation = screen.getByRole("navigation", { name: "Settings sections" });
    expect(Array.from(navigation.querySelectorAll("button")).map((button) => button.textContent)).toEqual([
      "General", "Widget", "Appearance", "Version & Updates",
    ]);
    expect(screen.getByRole("combobox", { name: "Language" })).toHaveValue("en");
    expect(screen.getByRole("checkbox", { name: "Launch at login" })).not.toBeChecked();
    expect(screen.getByRole("spinbutton", { name: "Auto-rotation interval" })).toHaveValue(12);

    fireEvent.click(screen.getByRole("button", { name: "Widget" }));
    expect(screen.queryByRole("checkbox", { name: "Always on top" })).toBeNull();
    expect(screen.getByRole("checkbox", { name: "Click-through" })).not.toBeChecked();
    expect(screen.getByRole("spinbutton", { name: "Compact size" })).toHaveValue(72);
    expect(screen.getByRole("spinbutton", { name: "Expanded size" })).toHaveValue(306);
    for (const preset of ["Small", "Medium", "Large"]) expect(screen.getByRole("button", { name: preset })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
    for (const appearance of ["System", "Light", "Dark"]) expect(screen.getByRole("radio", { name: appearance })).toBeInTheDocument();
    expect(Array.from(screen.getByTestId("settings-panel").querySelectorAll(".appearance-settings > section > h3")).map((heading) => heading.textContent)).toEqual(["Theme", "Skins"]);

    for (const skin of ["Default", "Soft Light", "Computer"]) expect(screen.getByRole("radio", { name: skin })).toBeInTheDocument();
    const skinGrid = screen.getByText("Default").closest(".skin-grid");
    expect(Array.from(skinGrid?.querySelectorAll(".skin-choice") ?? []).map((choice) => choice.textContent?.trim())).toEqual(["Default", "Soft Light", "Computer"]);
    for (const style of ["Transparent", "Dock frosted glass", "Liquid Glass"]) {
      expect(screen.getByRole("radio", { name: style })).toBeInTheDocument();
    }
    expect(screen.getByRole("radio", { name: "Dock frosted glass" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Liquid Glass" })).toBeDisabled();
    expect(screen.getByText("Requires macOS 26")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: "Transparent" }));
    await waitFor(() => expect(bridge.updatePreferences).toHaveBeenCalledWith(expect.objectContaining({ glassStyle: "transparent" })));
    expect(screen.getByLabelText("Import custom skin")).toHaveAttribute("accept", ".png,.jpg,.jpeg,.webp,image/png,image/jpeg,image/webp");
    expect(screen.queryByRole("button", { name: /Save/i })).toBeNull();
  });

  it("enables native Liquid Glass only when the platform reports support", async () => {
    bridge.getPlatformCapabilities.mockResolvedValueOnce({ nativeGlass: true, supportsLiquidGlass: true });
    await renderSettings({ ...basePreferences, selectedSkin: "glass" });
    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
    const liquid = screen.getByRole("radio", { name: "Liquid Glass" });
    expect(liquid).toBeEnabled();
    fireEvent.click(liquid);
    await waitFor(() => expect(bridge.updatePreferences).toHaveBeenCalledWith(expect.objectContaining({ glassStyle: "liquid" })));
  });

  it("applies general, widget, and appearance changes immediately", async () => {
    await renderSettings();

    fireEvent.change(screen.getByRole("spinbutton", { name: "Auto-rotation interval" }), { target: { value: "20" } });
    await waitFor(() => expect(bridge.updatePreferences).toHaveBeenCalledWith(expect.objectContaining({ autoRotateSeconds: 20 })));
    fireEvent.click(screen.getByRole("checkbox", { name: "Launch at login" }));
    await waitFor(() => expect(bridge.setLaunchAtLogin).toHaveBeenCalledWith(true));

    fireEvent.click(screen.getByRole("button", { name: "Widget" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Click-through" }));
    fireEvent.change(screen.getByRole("spinbutton", { name: "Compact size" }), { target: { value: "96" } });
    fireEvent.change(screen.getByRole("spinbutton", { name: "Expanded size" }), { target: { value: "360" } });
    fireEvent.click(screen.getByRole("button", { name: "Large" }));
    await waitFor(() => {
      expect(bridge.setClickThrough).toHaveBeenCalledWith(true);
      expect(bridge.setWidgetDimensions).toHaveBeenCalledWith(96, 360);
      expect(bridge.setWidgetSize).toHaveBeenCalledWith("large");
    });

    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    await waitFor(() => expect(bridge.updatePreferences).toHaveBeenCalledWith(expect.objectContaining({ appearance: "dark" })));

    fireEvent.click(screen.getByRole("radio", { name: "Computer" }));
    await waitFor(() => expect(bridge.selectSkin).toHaveBeenCalledWith("computer"));
    fireEvent.click(screen.getByRole("radio", { name: "Default" }));
    await waitFor(() => expect(bridge.selectSkin).toHaveBeenCalledWith("glass"));
  });

  it("shows the version and controls automatic update checks", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Version & Updates" }));
    expect(screen.getByText("v1.0.0")).toBeInTheDocument();
    const automatic = screen.getByRole("checkbox", { name: "Automatically check for updates" });
    expect(automatic).toBeChecked();
    fireEvent.click(automatic);
    await waitFor(() => expect(bridge.updatePreferences).toHaveBeenCalledWith(expect.objectContaining({ autoCheckUpdates: false })));
    fireEvent.click(screen.getByRole("button", { name: "Check for updates now" }));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Quota Pro is up to date"));
    const openProject = vi.spyOn(window, "open").mockImplementation(() => null);
    fireEvent.click(screen.getByRole("button", { name: "Project address" }));
    expect(openProject).toHaveBeenCalledWith("https://github.com/komakizhu/Quota-Pro", "_blank", "noopener,noreferrer");
    openProject.mockRestore();
  });

  it("switches the settings language through the same live preference seam", async () => {
    await renderSettings();
    fireEvent.change(screen.getByRole("combobox", { name: "Language" }), { target: { value: "zh-CN" } });
    await waitFor(() => expect(bridge.updatePreferences).toHaveBeenCalledWith(expect.objectContaining({ language: "zh-CN" })));
    expect(await screen.findByRole("heading", { name: "通用" })).toBeInTheDocument();
  });

  it("accepts live native preference events and applies deterministic contrast classes", async () => {
    await renderSettings();
    expect(bridge.preferencesHandler).not.toBeNull();

    bridge.preferencesHandler?.({ ...basePreferences, appearance: "dark" });
    await waitFor(() => expect(screen.getByTestId("settings-panel")).toHaveClass("settings-panel--dark"));
    bridge.preferencesHandler?.({ ...basePreferences, appearance: "light" });
    await waitFor(() => expect(screen.getByTestId("settings-panel")).toHaveClass("settings-panel--light"));
  });

  it("keeps the launch toggle aligned with verified native state and restores it on failure", async () => {
    await renderSettings();
    const launch = screen.getByRole("checkbox", { name: "Launch at login" });

    bridge.launchHandler?.(true);
    await waitFor(() => expect(launch).toBeChecked());

    bridge.setLaunchAtLogin.mockRejectedValueOnce(new Error("permission denied"));
    bridge.getLaunchAtLogin.mockResolvedValueOnce(true);
    fireEvent.click(launch);
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("permission denied"));
    expect(launch).toBeChecked();
  });

  it("keeps an unreadable launch-at-login state unknown instead of showing it as off", async () => {
    bridge.getLaunchAtLogin.mockRejectedValueOnce(new Error("state unavailable"));
    await renderSettings();

    const launch = screen.getByRole("checkbox", { name: "Launch at login" });
    expect(launch).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent("state unavailable");
  });
});

describe("SettingsPanel custom skins", () => {
  it("imports a supported image and exposes it for immediate selection", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
    const file = new File([new Uint8Array([1, 2, 3])], "lake.png", { type: "image/png" });
    Object.defineProperty(file, "arrayBuffer", { value: async () => new Uint8Array([1, 2, 3]).buffer });
    fireEvent.change(screen.getByLabelText("Import custom skin"), { target: { files: [file] } });

    expect(await screen.findByRole("radio", { name: "Lake" })).toBeInTheDocument();
    expect(bridge.importCustomSkin).toHaveBeenCalledWith("lake.png", new Uint8Array([1, 2, 3]));
    expect(screen.getByRole("status")).toHaveTextContent("Custom skin imported");
  });

  it("reports import failures inline", async () => {
    bridge.importCustomSkin.mockRejectedValueOnce(new Error("unsupported image"));
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
    const input = screen.getByLabelText("Import custom skin");
    const file = new File([new Uint8Array([1, 2, 3])], "bad.svg", { type: "image/svg+xml" });
    Object.defineProperty(file, "arrayBuffer", { value: async () => new Uint8Array([1, 2, 3]).buffer });
    fireEvent.change(input, { target: { files: [file] } });

    expect(await screen.findByRole("alert")).toHaveTextContent("unsupported image");
  });

  it("loads, selects, edits, and deletes a custom skin with active-skin fallback", async () => {
    const withCustom = { ...basePreferences, selectedSkin: "default", customSkins: [customSkin] };
    bridge.deleteCustomSkin.mockResolvedValueOnce({ ...withCustom, selectedSkin: "default", customSkins: [] });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    await renderSettings(withCustom);
    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));

    const customChoice = await screen.findByRole("radio", { name: "Lake" });
    const preview = screen.getByTestId(`skin-preview-${customSkin.id}`);
    await waitFor(() => expect(preview).toHaveStyle({ backgroundImage: "url(data:image/png;base64,LAKE)", backgroundSize: "cover", backgroundPosition: "center" }));
    expect(preview).toHaveClass("skin-preview--text-light");
    fireEvent.click(customChoice);
    await waitFor(() => expect(bridge.selectSkin).toHaveBeenCalledWith(`custom:${customSkin.id}`));

    const name = screen.getByRole("textbox", { name: "Skin name" });
    fireEvent.change(name, { target: { value: "Ocean" } });
    fireEvent.blur(name);
    await waitFor(() => expect(bridge.updateCustomSkin).toHaveBeenCalledWith(customSkin.id, "Ocean", "auto", "#123456"));
    fireEvent.change(screen.getByRole("combobox", { name: "Text tone" }), { target: { value: "dark" } });
    await waitFor(() => expect(bridge.updateCustomSkin).toHaveBeenCalledWith(customSkin.id, "Ocean", "dark", "#123456"));
    fireEvent.change(screen.getByLabelText("Accent color"), { target: { value: "#654321" } });
    await waitFor(() => expect(bridge.updateCustomSkin).toHaveBeenCalledWith(customSkin.id, "Ocean", "dark", "#654321"));

    fireEvent.click(screen.getByRole("button", { name: "Delete Ocean" }));
    await waitFor(() => expect(bridge.deleteCustomSkin).toHaveBeenCalledWith(customSkin.id));
    expect(screen.getByRole("radio", { name: "Soft Light" })).toBeChecked();
    expect(screen.queryByRole("radio", { name: "Ocean" })).toBeNull();
  });
});

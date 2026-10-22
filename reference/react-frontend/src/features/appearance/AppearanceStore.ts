import { useEffect, useState } from 'react';
import { appStorage } from '../../shared/storage/appStorage';

export const PALETTES = ['abyssal', 'ion', 'ember', 'bloom', 'forest', 'light'] as const;
export type Palette = typeof PALETTES[number];

export const DENSITIES = ['compact', 'cozy', 'roomy'] as const;
export type Density = typeof DENSITIES[number];

const PALETTE_KEY = 'plasma_appearance_palette';
const DENSITY_KEY = 'plasma_appearance_density';
const APPEARANCE_EVENT = 'plasma-appearance-changed';

function isPalette(value: string | null): value is Palette {
  return Boolean(value && (PALETTES as readonly string[]).includes(value));
}

function isDensity(value: string | null): value is Density {
  return Boolean(value && (DENSITIES as readonly string[]).includes(value));
}

export function getStoredPalette(): Palette {
  const storedPalette = appStorage.get(PALETTE_KEY);
  return isPalette(storedPalette)
    ? storedPalette
    : 'abyssal';
}

export function getStoredDensity(): Density {
  const storedDensity = appStorage.get(DENSITY_KEY);
  return isDensity(storedDensity)
    ? storedDensity
    : 'cozy';
}

export function setStoredPalette(palette: Palette) {
  appStorage.set(PALETTE_KEY, palette);
  window.dispatchEvent(new CustomEvent(APPEARANCE_EVENT));
}

export function setStoredDensity(density: Density) {
  appStorage.set(DENSITY_KEY, density);
  window.dispatchEvent(new CustomEvent(APPEARANCE_EVENT));
}

export function useAppearance() {
  const [palette, setPaletteState] = useState<Palette>(getStoredPalette);
  const [density, setDensityState] = useState<Density>(getStoredDensity);

  useEffect(() => {
    const sync = () => {
      setPaletteState(getStoredPalette());
      setDensityState(getStoredDensity());
    };

    window.addEventListener(APPEARANCE_EVENT, sync);
    window.addEventListener('storage', sync);
    return () => {
      window.removeEventListener(APPEARANCE_EVENT, sync);
      window.removeEventListener('storage', sync);
    };
  }, []);

  return {
    palette,
    density,
    setPalette: setStoredPalette,
    setDensity: setStoredDensity,
  };
}

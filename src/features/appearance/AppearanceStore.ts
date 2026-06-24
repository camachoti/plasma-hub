import { useEffect, useState } from 'react';

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
  return isPalette(localStorage.getItem(PALETTE_KEY))
    ? (localStorage.getItem(PALETTE_KEY) as Palette)
    : 'abyssal';
}

export function getStoredDensity(): Density {
  return isDensity(localStorage.getItem(DENSITY_KEY))
    ? (localStorage.getItem(DENSITY_KEY) as Density)
    : 'cozy';
}

export function setStoredPalette(palette: Palette) {
  localStorage.setItem(PALETTE_KEY, palette);
  window.dispatchEvent(new CustomEvent(APPEARANCE_EVENT));
}

export function setStoredDensity(density: Density) {
  localStorage.setItem(DENSITY_KEY, density);
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

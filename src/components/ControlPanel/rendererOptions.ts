import { RenderDriverSupport } from '../../hooks/useScrcpy';

export interface RendererOption {
    value: string;
    label: string;
}

export function buildRendererOptions(
    renderDriverSupport: RenderDriverSupport,
    autoLabel: string = 'Auto'
): RendererOption[] {
    return [
        { value: 'auto', label: autoLabel },
        ...renderDriverSupport.supportedDrivers.map((driver) => ({
            value: driver.id,
            label: driver.label
        }))
    ];
}

export function mapRendererSelection(value: string): string | undefined {
    return value === 'auto' ? undefined : value;
}

/** Host-side hardware decoders supported by scrcpy v5+, filtered to the
 *  platforms where each API actually exists. */
export function buildHwdecOptions(
    hostOs: string,
    autoLabel: string = 'Auto',
    disabledLabel: string = 'Disabled'
): RendererOption[] {
    const options: RendererOption[] = [
        { value: 'auto', label: autoLabel },
        { value: 'disabled', label: disabledLabel }
    ];

    if (hostOs === 'windows') {
        options.push({ value: 'd3d11va', label: 'D3D11VA' });
    } else if (hostOs === 'linux') {
        options.push({ value: 'vaapi', label: 'VAAPI' });
    } else if (hostOs === 'macos') {
        options.push({ value: 'videotoolbox', label: 'VideoToolbox' });
    }

    return options;
}

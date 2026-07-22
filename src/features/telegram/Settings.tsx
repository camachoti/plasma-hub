import React, { useEffect, useState } from 'react';
import { telegramService } from './TelegramService';
import { Trash } from '@phosphor-icons/react';
import { DENSITIES, PALETTES, useAppearance } from '../appearance/AppearanceStore';
import { getStoredTwitterCookies, setStoredTwitterCookies } from '../twitter/TwitterSettingsStore';
import { appStorage } from '../../shared/storage/appStorage';

interface CacheStats {
  totalSize: number;
  messageCount: number;
  mediaCount: number;
  avatarCount: number;
  nativeTotalSize?: number;
  nativeMediaCount?: number;
}

interface CacheSettings {
  maxCacheSize: number;
  avatarRefreshHours: number;
  downloadWorkers: number;
}

const SIZE_OPTIONS = [
  { label: 'Sem limite', value: 0 },
  { label: '100 MB', value: 100 * 1024 * 1024 },
  { label: '500 MB', value: 500 * 1024 * 1024 },
  { label: '1 GB', value: 1024 * 1024 * 1024 },
  { label: '2 GB', value: 2 * 1024 * 1024 * 1024 },
];

const AVATAR_REFRESH_OPTIONS = [
  { label: '1 hora', value: 1 },
  { label: '6 horas', value: 6 },
  { label: '24 horas', value: 24 },
  { label: '72 horas', value: 72 },
  { label: 'Nunca', value: 999999 },
];

const WORKER_OPTIONS = [
  { label: '1 conexão (Lento / Mais Seguro)', value: 1 },
  { label: '2 conexões (Equilibrado)', value: 2 },
  { label: '4 conexões (Padrão)', value: 4 },
  { label: '8 conexões (Rápido - Recomendado Premium)', value: 8 },
  { label: '12 conexões (Muito Rápido)', value: 12 },
  { label: '16 conexões (Velocidade Máxima)', value: 16 },
];

function formatBytes(bytes: number) {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

interface SettingsProps {
  onClose: () => void;
}

type SettingsTab = 'general' | 'visual' | 'cache';

export const Settings: React.FC<SettingsProps> = ({ onClose }) => {
  const { palette, density, setPalette, setDensity } = useAppearance();
  const [activeTab, setActiveTab] = useState<SettingsTab>('general');
  const [stats, setStats] = useState<CacheStats | null>(null);
  const [settings, setSettings] = useState<CacheSettings>({ maxCacheSize: 0, avatarRefreshHours: 24, downloadWorkers: 4 });
  const [twitterCookies, setTwitterCookies] = useState(getStoredTwitterCookies);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [clearConfirm, setClearConfirm] = useState(false);
  const [clearTwitter, setClearTwitter] = useState(() => appStorage.getBoolean("clear_twitter_groups_on_clear_cache"));
  const [tdlibState, setTdlibState] = useState('not_initialized');
  const [tdlibMessage, setTdlibMessage] = useState('');
  const [tdlibPhone, setTdlibPhone] = useState('');
  const [tdlibCode, setTdlibCode] = useState('');
  const [tdlibPassword, setTdlibPassword] = useState('');
  const [tdlibBusy, setTdlibBusy] = useState(false);

  useEffect(() => { loadData(); }, []);

  const loadData = async () => {
    setLoading(true);
    try {
      const [statsRes, settingsRes] = await Promise.all([
        telegramService.getCacheStats(),
        telegramService.getCacheSettings(),
      ]);
      telegramService.tdlibStatus()
        .then((res: any) => {
          setTdlibState(res?.state || 'not_initialized');
          if (res?.error) setTdlibMessage(res.error);
        })
        .catch(() => {});
      if (statsRes.success) {
        setStats({ totalSize: statsRes.totalSize, messageCount: statsRes.messageCount, mediaCount: statsRes.mediaCount, avatarCount: statsRes.avatarCount });
      }
      if (settingsRes.success) {
        setSettings({
          maxCacheSize: settingsRes.maxCacheSize,
          avatarRefreshHours: settingsRes.avatarRefreshHours,
          downloadWorkers: settingsRes.downloadWorkers
        });
      }
    } finally {
      setLoading(false);
    }
  };

  const runTdlibAction = async (action: () => Promise<any>, successMessage: string) => {
    setTdlibBusy(true);
    setTdlibMessage('');
    try {
      const res = await action();
      setTdlibState(res?.state || (res?.ready ? 'ready' : tdlibState));
      setTdlibMessage(res?.error || successMessage);
      return res;
    } catch (error: any) {
      setTdlibMessage(error?.message || String(error));
      return null;
    } finally {
      setTdlibBusy(false);
    }
  };

  const handleTdlibInit = () => runTdlibAction(() => telegramService.tdlibInit(), 'TDLib inicializado.');
  const handleTdlibPhone = () => runTdlibAction(() => telegramService.tdlibSetPhone(tdlibPhone), 'Telefone enviado.');
  const handleTdlibCode = () => runTdlibAction(() => telegramService.tdlibCheckCode(tdlibCode), 'Código validado.');
  const handleTdlibPassword = () => runTdlibAction(() => telegramService.tdlibCheckPassword(tdlibPassword), 'Senha validada.');
  const handleTdlibTest = async () => {
    setTdlibBusy(true);
    setTdlibMessage('');
    try {
      const res: any = await telegramService.tdlibGetMe();
      setTdlibMessage(res?.success ? `TDLib conectado como ${[res.first_name, res.last_name].filter(Boolean).join(' ') || res.id}` : (res?.error || 'TDLib não conectado.'));
    } catch (error: any) {
      setTdlibMessage(error?.message || String(error));
    } finally {
      setTdlibBusy(false);
    }
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      await telegramService.setCacheSettings(settings);
      setStoredTwitterCookies(twitterCookies);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } finally {
      setSaving(false);
    }
  };

  const handleClearCache = async () => {
    if (!clearConfirm) {
      setClearConfirm(true);
      return;
    }
    setClearing(true);
    try {
      await telegramService.clearCache();
      if (clearTwitter) {
        appStorage.remove('plasma_twitter_fake_chats');
        appStorage.remove('plasma_twitter_pending_fake_chat');
        window.dispatchEvent(new CustomEvent('plasma-twitter-fake-chat-created'));
      }
      setClearConfirm(false);
      await loadData();
    } finally {
      setClearing(false);
    }
  };

  return (
    <div className="settings-overlay" onClick={onClose}>
      <div className="settings-panel" onClick={e => e.stopPropagation()}>
        <div className="settings-header">
          <h2>Configurações Gerais</h2>
          <button className="icon-btn" onClick={onClose}>✕</button>
        </div>
        <div className="settings-tabs" role="tablist" aria-label="Seções de configurações">
          <button
            type="button"
            className={`settings-tab ${activeTab === 'general' ? 'active' : ''}`}
            onClick={() => setActiveTab('general')}
            role="tab"
            aria-selected={activeTab === 'general'}
          >
            Geral
          </button>
          <button
            type="button"
            className={`settings-tab ${activeTab === 'visual' ? 'active' : ''}`}
            onClick={() => setActiveTab('visual')}
            role="tab"
            aria-selected={activeTab === 'visual'}
          >
            Visual
          </button>
          <button
            type="button"
            className={`settings-tab ${activeTab === 'cache' ? 'active' : ''}`}
            onClick={() => setActiveTab('cache')}
            role="tab"
            aria-selected={activeTab === 'cache'}
          >
            Cache
          </button>
        </div>

        {loading ? (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 40 }}>
            <span className="spinner" />
          </div>
        ) : (
          <div className="settings-body">
            {activeTab === 'general' && (
              <>
                <div className="settings-section">
                  <h3>Twitter/X</h3>
                  <div className="settings-field settings-field-stack">
                    <label htmlFor="twitter-cookies">Cookies opcionais</label>
                    <textarea
                      id="twitter-cookies"
                      value={twitterCookies}
                      onChange={e => setTwitterCookies(e.target.value)}
                      className="settings-textarea"
                      placeholder="Cookie: auth_token=...; ct0=... ou cole o conteudo/caminho do cookies.txt"
                      spellCheck={false}
                    />
                  </div>
                  <p className="settings-hint">
                    Esses cookies são usados na análise e download de tweets ou perfis quando o Twitter/X exige sessão.
                  </p>
                </div>

                <div className="settings-section">
                  <h3>Telegram nativo</h3>
                  <div className="settings-field">
                    <label>Status TDLib</label>
                    <span className="settings-hint" style={{ margin: 0 }}>{tdlibState}</span>
                  </div>
                  <div className="settings-field">
                    <label>Telefone</label>
                    <input
                      type="tel"
                      value={tdlibPhone}
                      onChange={e => setTdlibPhone(e.target.value)}
                      placeholder="+55..."
                    />
                  </div>
                  <div className="settings-field">
                    <label>Código</label>
                    <input
                      value={tdlibCode}
                      onChange={e => setTdlibCode(e.target.value)}
                      placeholder="12345"
                    />
                  </div>
                  <div className="settings-field">
                    <label>Senha 2FA</label>
                    <input
                      type="password"
                      value={tdlibPassword}
                      onChange={e => setTdlibPassword(e.target.value)}
                      placeholder="Opcional"
                    />
                  </div>
                  <div className="settings-actions" style={{ justifyContent: 'flex-start', paddingTop: 8 }}>
                    <button className="settings-save-btn" type="button" onClick={handleTdlibInit} disabled={tdlibBusy}>
                      Inicializar
                    </button>
                    <button className="settings-save-btn" type="button" onClick={handleTdlibPhone} disabled={tdlibBusy || !tdlibPhone}>
                      Enviar telefone
                    </button>
                    <button className="settings-save-btn" type="button" onClick={handleTdlibCode} disabled={tdlibBusy || !tdlibCode}>
                      Validar código
                    </button>
                    <button className="settings-save-btn" type="button" onClick={handleTdlibPassword} disabled={tdlibBusy || !tdlibPassword}>
                      Validar senha
                    </button>
                    <button className="settings-save-btn" type="button" onClick={handleTdlibTest} disabled={tdlibBusy}>
                      Testar
                    </button>
                  </div>
                  {tdlibMessage && <p className="settings-hint">{tdlibMessage}</p>}
                </div>
              </>
            )}

            {activeTab === 'visual' && (
              <div className="settings-section">
                <h3>Aparência</h3>
                <div className="settings-field settings-field-stack">
                  <label>Paleta</label>
                  <div className="settings-palette-picker">
                    {PALETTES.map(p => (
                      <button
                        key={p}
                        className={`palette-dot ${p} ${palette === p ? 'active' : ''}`}
                        onClick={() => setPalette(p)}
                        title={p}
                        type="button"
                      />
                    ))}
                  </div>
                </div>
                <div className="settings-field settings-field-stack">
                  <label>Densidade</label>
                  <div className="settings-density-row">
                    {DENSITIES.map(d => (
                      <button
                        key={d}
                        className={`density-btn ${density === d ? 'active' : ''}`}
                        onClick={() => setDensity(d)}
                        type="button"
                      >
                        {d}
                      </button>
                    ))}
                  </div>
                </div>
                <p className="settings-hint">
                  As mudanças visuais são aplicadas imediatamente em todo o aplicativo.
                </p>
              </div>
            )}

            {activeTab === 'cache' && (
              <>
                {stats && (
                  <div className="settings-section">
                    <h3>Estatísticas</h3>
                    <div className="settings-stats-grid" style={{ gridTemplateColumns: 'repeat(2, 1fr)' }}>
                      <div className="settings-stat">
                        <span className="settings-stat-value">{formatBytes(stats.totalSize)}</span>
                        <span className="settings-stat-label">Espaço total</span>
                      </div>
                      <div className="settings-stat">
                        <span className="settings-stat-value">{stats.messageCount.toLocaleString()}</span>
                        <span className="settings-stat-label">Mensagens</span>
                      </div>
                      <div className="settings-stat">
                        <span className="settings-stat-value">{stats.mediaCount.toLocaleString()}</span>
                        <span className="settings-stat-label">Arquivos de mídia</span>
                      </div>
                      <div className="settings-stat">
                        <span className="settings-stat-value">{formatBytes(stats.nativeTotalSize || 0)}</span>
                        <span className="settings-stat-label">Cache nativo</span>
                      </div>
                      <div className="settings-stat">
                        <span className="settings-stat-value">{stats.avatarCount.toLocaleString()}</span>
                        <span className="settings-stat-label">Avatares</span>
                      </div>
                    </div>
                  </div>
                )}

                <div className="settings-section">
                  <h3>Limite de Cache</h3>
                  <div className="settings-field">
                    <label>Tamanho máximo</label>
                    <select
                      value={settings.maxCacheSize}
                      onChange={e => setSettings(s => ({ ...s, maxCacheSize: Number(e.target.value) }))}
                    >
                      {SIZE_OPTIONS.map(o => (
                        <option key={o.value} value={o.value}>{o.label}</option>
                      ))}
                    </select>
                  </div>
                  <p className="settings-hint">
                    Quando o limite é atingido, os arquivos de mídia menos utilizados são removidos automaticamente (LRU).
                  </p>
                </div>

                <div className="settings-section">
                  <h3>Atualização de Avatares</h3>
                  <div className="settings-field">
                    <label>Intervalo de atualização</label>
                    <select
                      value={settings.avatarRefreshHours}
                      onChange={e => setSettings(s => ({ ...s, avatarRefreshHours: Number(e.target.value) }))}
                    >
                      {AVATAR_REFRESH_OPTIONS.map(o => (
                        <option key={o.value} value={o.value}>{o.label}</option>
                      ))}
                    </select>
                  </div>
                </div>

                <div className="settings-section">
                  <h3>Desempenho de Download</h3>
                  <div className="settings-field">
                    <label>Conexões Simultâneas (Workers)</label>
                    <select
                      value={settings.downloadWorkers}
                      onChange={e => setSettings(s => ({ ...s, downloadWorkers: Number(e.target.value) }))}
                    >
                      {WORKER_OPTIONS.map(o => (
                        <option key={o.value} value={o.value}>{o.label}</option>
                      ))}
                    </select>
                  </div>
                  <p className="settings-hint">
                    Aumentar as conexões simultâneas acelera drasticamente o Mass Download, permitindo extrair a velocidade máxima de sua conta Telegram Premium.
                  </p>
                </div>

                <div className="settings-section">
                  <h3>Limpeza do Twitter/X</h3>
                  <div className="settings-field">
                    <label>Limpar chats importados do Twitter/X</label>
                    <label className="switch-label" style={{ margin: 0 }}>
                      <input
                        type="checkbox"
                        checked={clearTwitter}
                        onChange={e => {
                          const val = e.target.checked;
                          setClearTwitter(val);
                          appStorage.setBoolean("clear_twitter_groups_on_clear_cache", val);
                        }}
                      />
                      <span className="switch-custom" />
                    </label>
                  </div>
                  <p className="settings-hint">
                    Se ativado, ao limpar o cache do aplicativo, todos os chats falsos e mídias importadas do Twitter/X serão excluídos permanentemente.
                  </p>
                </div>
              </>
            )}

            <div className="settings-actions">
              <button className="settings-save-btn" onClick={handleSave} disabled={saving}>
                {saved ? '✓ Salvo!' : saving ? 'Salvando...' : 'Salvar configurações'}
              </button>
              {activeTab === 'cache' && (
                <div className="settings-clear-wrap">
                  {clearConfirm && (
                    <button type="button" className="settings-cancel-btn" onClick={() => setClearConfirm(false)}>
                      Cancelar
                    </button>
                  )}
                  <button
                    className={`settings-clear-btn${clearConfirm ? ' confirm' : ''}`}
                    onClick={handleClearCache}
                    disabled={clearing}
                  >
                    {clearing ? 'Limpando...' : clearConfirm ? '⚠️ Confirmar limpeza' : <><Trash size={16} style={{ verticalAlign: 'text-bottom', marginRight: 6 }} />Limpar cache</>}
                  </button>
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
};

/**
 * settings.json persistence.
 *
 * The AI provider key is never written in plain text when the OS offers
 * Electron safeStorage (Keychain, DPAPI, libsecret/kwallet): it is stored as
 * `aiKeyEnc` and decrypted only in the main process. Without such storage
 * the key stays in a file readable only by the user (0600). Writes are
 * atomic (temp file + rename) so a crash cannot leave half a JSON file.
 */
const fs = require('fs')
const path = require('path')

const SECRET_FIELDS = ['aiKey']

/**
 * @param {object} settings  as received from the renderer
 * @param {{available:boolean, encrypt:(s:string)=>string}} codec
 */
function encodeSettingsForDisk(settings, codec) {
  const out = { ...settings }
  for (const field of SECRET_FIELDS) {
    const value = typeof out[field] === 'string' ? out[field] : ''
    delete out[`${field}Enc`]
    if (!value) {
      out[field] = ''
      continue
    }
    if (codec?.available) {
      out[`${field}Enc`] = codec.encrypt(value)
      out[field] = ''
    }
  }
  return out
}

/**
 * @returns {{settings:object, needsRewrite:boolean}}
 */
function decodeSettingsFromDisk(raw, codec) {
  const settings = { ...raw }
  let needsRewrite = false
  for (const field of SECRET_FIELDS) {
    const enc = settings[`${field}Enc`]
    delete settings[`${field}Enc`]
    if (typeof enc === 'string' && enc) {
      try {
        settings[field] = codec?.available ? codec.decrypt(enc) : ''
      } catch {
        // Keyring changed or unavailable: the key cannot be recovered
        settings[field] = ''
      }
    } else if (typeof settings[field] === 'string' && settings[field] && codec?.available) {
      needsRewrite = true // legacy plain-text key
    }
  }
  return { settings, needsRewrite }
}

function writeFileAtomic(file, data) {
  const dir = path.dirname(file)
  const tmp = path.join(dir, `.${path.basename(file)}.${process.pid}.${Date.now()}.tmp`)
  const fd = fs.openSync(tmp, 'w', 0o600)
  try {
    fs.writeFileSync(fd, data, 'utf8')
    try { fs.fsyncSync(fd) } catch {}
  } finally {
    fs.closeSync(fd)
  }
  try {
    fs.renameSync(tmp, file)
  } catch (err) {
    try { fs.unlinkSync(tmp) } catch {}
    throw err
  }
  try { fs.chmodSync(file, 0o600) } catch {}
}

module.exports = { encodeSettingsForDisk, decodeSettingsFromDisk, writeFileAtomic, SECRET_FIELDS }

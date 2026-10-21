/**
 * Strict TLS Certificate Validator
 * Provides production-grade certificate validation with pinning support
 *
 * This module implements comprehensive certificate validation including:
 * - Certificate date validation
 * - Hostname verification (CN and SAN)
 * - Key strength requirements
 * - Certificate pinning support
 * - Wildcard certificate matching
 */
import crypto from 'crypto';
import tls from 'tls';

class StrictCertificateValidator {
  constructor() {
    this.pinnedCertificates = new Map();
    this.minKeyBits = 2048;
  }

  /**
   * Validate certificate with strict security checks
   * @param {Object} cert - Certificate object
   * @param {string} hostname - Expected hostname
   * @returns {boolean} True if valid
   * @throws {Error} If validation fails
   */
  validateCertificate(cert, hostname) {
    // 1. Check certificate dates
    const now = new Date();
    if (now < new Date(cert.valid_from)) {
      throw new Error(
        `Certificate not yet valid: valid from ${cert.valid_from}`
      );
    }
    if (now > new Date(cert.valid_to)) {
      throw new Error(`Certificate expired: valid until ${cert.valid_to}`);
    }

    // 2. Validate hostname matches CN or SAN
    if (!this.validateHostname(cert, hostname)) {
      throw new Error(`Certificate hostname mismatch: expected ${hostname}`);
    }

    // 3. Check key strength
    if (cert.bits && cert.bits < this.minKeyBits) {
      throw new Error(
        `Certificate key too weak: ${cert.bits} bits < ${this.minKeyBits} minimum`
      );
    }

    // 4. Check certificate pinning if configured
    if (this.pinnedCertificates.has(hostname)) {
      const pinnedFingerprint = this.pinnedCertificates.get(hostname);
      const certFingerprint = this.calculateFingerprint(cert);

      if (certFingerprint !== pinnedFingerprint) {
        throw new Error(`Certificate pin mismatch for ${hostname}`);
      }
    }

    return true;
  }

  /**
   * Validate hostname against certificate CN and SANs
   * @param {Object} cert - Certificate object
   * @param {string} hostname - Hostname to validate
   * @returns {boolean} True if hostname matches
   */
  validateHostname(cert, hostname) {
    // Check Common Name
    if (cert.subject && cert.subject.CN === hostname) {
      return true;
    }

    // Check Subject Alternative Names
    if (cert.subjectaltname) {
      const altNames = cert.subjectaltname.split(', ');
      for (const name of altNames) {
        if (name.startsWith('DNS:')) {
          const dnsName = name.slice(4);
          if (dnsName === hostname || this.matchWildcard(dnsName, hostname)) {
            return true;
          }
        }
      }
    }

    return false;
  }

  /**
   * Match wildcard certificates (e.g., *.example.com)
   * Wildcards only match a single subdomain level per RFC 6125
   * @param {string} pattern - Wildcard pattern
   * @param {string} hostname - Hostname to match
   * @returns {boolean} True if matches
   */
  matchWildcard(pattern, hostname) {
    if (!pattern.startsWith('*.')) return false;
    const suffix = pattern.slice(1); // .example.com

    // Hostname must end with the suffix
    if (!hostname.endsWith(suffix)) return false;

    // Extract the part before the suffix
    const prefix = hostname.slice(0, hostname.length - suffix.length);

    // Wildcard only matches a single label (no dots in prefix)
    // e.g., *.example.com matches sub.example.com but not deep.sub.example.com
    return prefix.length > 0 && !prefix.includes('.');
  }

  /**
   * Calculate certificate fingerprint (SHA-256)
   * @param {Object} cert - Certificate object
   * @returns {string} Fingerprint hex string
   */
  calculateFingerprint(cert) {
    if (cert.raw) {
      return crypto.createHash('sha256').update(cert.raw).digest('hex');
    }
    // Fallback: hash the fingerprint if raw not available
    return (cert.fingerprint256 || cert.fingerprint || '')
      .toLowerCase()
      .replace(/:/g, '');
  }

  /**
   * Pin a certificate for a hostname
   * @param {string} hostname - Hostname to pin
   * @param {string} fingerprint - Certificate fingerprint (SHA-256)
   */
  pinCertificate(hostname, fingerprint) {
    this.pinnedCertificates.set(
      hostname,
      fingerprint.toLowerCase().replace(/:/g, '')
    );
  }

  /**
   * Remove certificate pin for a hostname
   * @param {string} hostname - Hostname to unpin
   */
  unpinCertificate(hostname) {
    this.pinnedCertificates.delete(hostname);
  }

  /**
   * Set minimum key strength requirement
   * @param {number} bits - Minimum key bits (must be >= 2048)
   * @throws {Error} If bits < 2048
   */
  setMinKeyBits(bits) {
    if (bits < 2048) {
      throw new Error('Minimum key bits cannot be less than 2048');
    }
    this.minKeyBits = bits;
  }

  /**
   * Get all pinned certificates
   * @returns {Object} Map of hostname to fingerprint
   */
  getPinnedCertificates() {
    const result = {};
    for (const [hostname, fingerprint] of this.pinnedCertificates) {
      result[hostname] = fingerprint;
    }
    return result;
  }

  /**
   * Create TLS options with strict validation
   * @param {string} hostname - Target hostname
   * @returns {Object} TLS connection options
   */
  createStrictTLSOptions(hostname) {
    const self = this;
    return {
      rejectUnauthorized: true,
      minVersion: 'TLSv1.2',
      checkServerIdentity: function (host, cert) {
        try {
          self.validateCertificate(cert, host);
          return undefined; // No error
        } catch (error) {
          return error; // Return validation error
        }
      },
    };
  }

  /**
   * Validate certificate chain
   * @param {Array} certChain - Array of certificates in chain
   * @returns {Object} Validation result with details
   */
  validateCertificateChain(certChain) {
    const results = {
      valid: true,
      chainLength: certChain.length,
      certificates: [],
      errors: [],
    };

    for (let i = 0; i < certChain.length; i++) {
      const cert = certChain[i];
      const certInfo = {
        index: i,
        subject: cert.subject ? cert.subject.CN : 'Unknown',
        issuer: cert.issuer ? cert.issuer.CN : 'Unknown',
        validFrom: cert.valid_from,
        validTo: cert.valid_to,
        bits: cert.bits,
        fingerprint: this.calculateFingerprint(cert),
      };

      // Validate date
      const now = new Date();
      if (now < new Date(cert.valid_from)) {
        results.valid = false;
        results.errors.push(
          `Certificate ${i} not yet valid: valid from ${cert.valid_from}`
        );
      }
      if (now > new Date(cert.valid_to)) {
        results.valid = false;
        results.errors.push(
          `Certificate ${i} expired: valid until ${cert.valid_to}`
        );
      }

      // Validate key strength
      if (cert.bits && cert.bits < this.minKeyBits) {
        results.valid = false;
        results.errors.push(
          `Certificate ${i} key too weak: ${cert.bits} bits < ${this.minKeyBits} minimum`
        );
      }

      results.certificates.push(certInfo);
    }

    return results;
  }

  /**
   * Extract certificate information for logging (no sensitive data)
   * @param {Object} cert - Certificate object
   * @returns {Object} Safe certificate info for logging
   */
  extractCertInfo(cert) {
    return {
      subject: cert.subject ? cert.subject.CN : 'Unknown',
      issuer: cert.issuer ? cert.issuer.CN : 'Unknown',
      validFrom: cert.valid_from,
      validTo: cert.valid_to,
      bits: cert.bits,
      serialNumber: cert.serialNumber,
      fingerprint: this.calculateFingerprint(cert),
      san: cert.subjectaltname || 'None',
    };
  }
}

export default StrictCertificateValidator;

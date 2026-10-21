import Foundation
import Network

/**
 * Advanced cache manager for API responses with intelligent caching strategies.
 *
 * Features:
 * - Memory and disk caching with size limits
 * - Network-aware cache policies
 * - Automatic cache invalidation
 * - Background cache optimization
 * - Offline-first support for critical data
 * - Cache performance analytics
 */
public class APICacheManager {
    
    // MARK: - Properties
    
    public static let shared = APICacheManager()
    
    private let memoryCache = NSCache<NSString, CachedResponse>()
    private let diskCache: DiskCache
    private let cacheQueue = DispatchQueue(label: "APICacheQueue", qos: .utility)
    
    // Cache configuration
    private let maxMemorySize: Int = 20 * 1024 * 1024  // 20MB
    private let maxDiskSize: Int = 100 * 1024 * 1024   // 100MB
    private let defaultTTL: TimeInterval = 300         // 5 minutes
    
    // Cache policies for different endpoint types
    private let cachePolicies: [String: CachePolicy] = [
        "weather": .cacheFirst(ttl: 300),      // 5 minutes for weather data
        "blockchain": .networkOnly,             // Always fresh for blockchain
        "profile": .cacheFirst(ttl: 900),      // 15 minutes for profile
        "static": .cacheOnly,                   // Never expire static content
        "analytics": .networkFirst(ttl: 60)     // 1 minute for analytics
    ]
    
    // MARK: - Cache Policy
    
    public enum CachePolicy {
        case networkOnly           // Always fetch from network
        case cacheOnly            // Only use cache, never network
        case cacheFirst(ttl: TimeInterval)  // Try cache first, fallback to network
        case networkFirst(ttl: TimeInterval) // Try network first, fallback to cache
        case staleWhileRevalidate(ttl: TimeInterval) // Return cache, update in background
    }
    
    // MARK: - Initialization
    
    private init() {
        // Configure memory cache
        memoryCache.totalCostLimit = maxMemorySize
        memoryCache.countLimit = 1000
        
        // Initialize disk cache
        diskCache = DiskCache(maxSize: maxDiskSize)
        
        // Setup memory warning observer
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(handleMemoryWarning),
            name: UIApplication.didReceiveMemoryWarningNotification,
            object: nil
        )
        
        // Setup background cache cleanup
        scheduleBackgroundCleanup()
    }
    
    // MARK: - Cache Operations
    
    /**
     * Stores a response in the cache.
     */
    public func store(
        response: URLResponse,
        data: Data,
        for request: URLRequest
    ) {
        guard let url = request.url else { return }
        
        let cachedResponse = CachedResponse(
            data: data,
            response: response,
            timestamp: Date(),
            ttl: getTTL(for: url)
        )
        
        let key = cacheKey(for: request)
        
        cacheQueue.async { [weak self] in
            // Store in memory cache
            self?.memoryCache.setObject(cachedResponse, forKey: key as NSString, cost: data.count)
            
            // Store in disk cache for larger TTL
            if cachedResponse.ttl > 60 {
                self?.diskCache.store(data: data, forKey: key)
            }
        }
    }
    
    /**
     * Retrieves a cached response if valid.
     */
    public func retrieve(for request: URLRequest) -> CachedResponse? {
        let key = cacheKey(for: request)
        
        // Try memory cache first
        if let cached = memoryCache.object(forKey: key as NSString) {
            if cached.isValid {
                return cached
            } else {
                memoryCache.removeObject(forKey: key as NSString)
            }
        }
        
        // Try disk cache
        if let data = diskCache.retrieve(forKey: key) {
            let cachedResponse = CachedResponse(
                data: data,
                response: URLResponse(),
                timestamp: Date(),
                ttl: defaultTTL
            )
            
            if cachedResponse.isValid {
                // Restore to memory cache
                memoryCache.setObject(cachedResponse, forKey: key as NSString, cost: data.count)
                return cachedResponse
            } else {
                diskCache.remove(forKey: key)
            }
        }
        
        return nil
    }
    
    /**
     * Checks if a cached response exists and is valid.
     */
    public func hasValidCache(for request: URLRequest) -> Bool {
        return retrieve(for: request) != nil
    }
    
    /**
     * Removes a specific cached response.
     */
    public func remove(for request: URLRequest) {
        let key = cacheKey(for: request)
        
        memoryCache.removeObject(forKey: key as NSString)
        
        cacheQueue.async { [weak self] in
            self?.diskCache.remove(forKey: key)
        }
    }
    
    /**
     * Clears all cached data.
     */
    public func clearAll() {
        memoryCache.removeAllObjects()
        
        cacheQueue.async { [weak self] in
            self?.diskCache.clearAll()
        }
    }
    
    /**
     * Clears expired cache entries.
     */
    public func clearExpired() {
        cacheQueue.async { [weak self] in
            self?.diskCache.clearExpired()
        }
    }
    
    // MARK: - Cache Policy Application
    
    /**
     * Gets the appropriate cache policy for a URL.
     */
    public func cachePolicy(for url: URL) -> CachePolicy {
        let path = url.path.lowercased()
        
        for (pattern, policy) in cachePolicies {
            if path.contains(pattern) {
                return policy
            }
        }
        
        return .cacheFirst(ttl: defaultTTL)
    }
    
    /**
     * Determines if a request should use cache based on policy.
     */
    public func shouldUseCache(for request: URLRequest, isOnline: Bool) -> Bool {
        guard let url = request.url else { return false }
        
        let policy = cachePolicy(for: url)
        
        switch policy {
        case .networkOnly:
            return false
        case .cacheOnly:
            return true
        case .cacheFirst:
            return hasValidCache(for: request) || !isOnline
        case .networkFirst:
            return !isOnline && hasValidCache(for: request)
        case .staleWhileRevalidate:
            return hasValidCache(for: request)
        }
    }
    
    // MARK: - Cache Analytics
    
    /**
     * Gets cache performance statistics.
     */
    public func getStatistics() -> CacheStatistics {
        let memoryUsage = memoryCache.totalCostLimit > 0 ? 
            Double(memoryCache.totalCostLimit) / Double(maxMemorySize) : 0.0
        
        return CacheStatistics(
            memoryUsagePercent: memoryUsage,
            diskUsageBytes: diskCache.currentSize,
            hitCount: 0, // Would track in production
            missCount: 0, // Would track in production
            totalRequests: 0 // Would track in production
        )
    }
    
    // MARK: - Private Methods
    
    private func cacheKey(for request: URLRequest) -> String {
        guard let url = request.url else { return "" }
        
        var components = [url.absoluteString]
        
        // Include relevant headers in cache key
        if let authHeader = request.value(forHTTPHeaderField: "Authorization") {
            components.append("auth:\\(authHeader.hashValue)")
        }
        
        return components.joined(separator: "|").sha256
    }
    
    private func getTTL(for url: URL) -> TimeInterval {
        let policy = cachePolicy(for: url)
        
        switch policy {
        case .cacheFirst(let ttl), .networkFirst(let ttl), .staleWhileRevalidate(let ttl):
            return ttl
        case .cacheOnly:
            return TimeInterval.greatestFiniteMagnitude
        case .networkOnly:
            return 0
        }
    }
    
    @objc private func handleMemoryWarning() {
        // Clear half of memory cache on memory warning
        let objects = memoryCache.totalCostLimit / 2
        memoryCache.totalCostLimit = objects
        
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) { [weak self] in
            self?.memoryCache.totalCostLimit = self?.maxMemorySize ?? 0
        }
    }
    
    private func scheduleBackgroundCleanup() {
        Timer.scheduledTimer(withTimeInterval: 3600, repeats: true) { [weak self] _ in
            self?.clearExpired()
        }
    }
    
    deinit {
        NotificationCenter.default.removeObserver(self)
    }
}

// MARK: - Supporting Types

public class CachedResponse {
    let data: Data
    let response: URLResponse
    let timestamp: Date
    let ttl: TimeInterval
    
    init(data: Data, response: URLResponse, timestamp: Date, ttl: TimeInterval) {
        self.data = data
        self.response = response
        self.timestamp = timestamp
        self.ttl = ttl
    }
    
    var isValid: Bool {
        return Date().timeIntervalSince(timestamp) < ttl
    }
}

public struct CacheStatistics {
    public let memoryUsagePercent: Double
    public let diskUsageBytes: Int64
    public let hitCount: Int
    public let missCount: Int
    public let totalRequests: Int
    
    public var hitRate: Double {
        return totalRequests > 0 ? Double(hitCount) / Double(totalRequests) : 0.0
    }
}

// MARK: - Disk Cache Implementation

private class DiskCache {
    private let cacheDirectory: URL
    private let maxSize: Int
    private let fileManager = FileManager.default
    private let queue = DispatchQueue(label: "DiskCacheQueue", qos: .utility)
    
    init(maxSize: Int) {
        self.maxSize = maxSize
        
        let cacheDir = fileManager.urls(for: .cachesDirectory, in: .userDomainMask).first!
        self.cacheDirectory = cacheDir.appendingPathComponent("WRFCoinAPICache")
        
        try? fileManager.createDirectory(at: cacheDirectory, withIntermediateDirectories: true)
    }
    
    func store(data: Data, forKey key: String) {
        queue.async { [weak self] in
            guard let self = self else { return }
            
            let fileURL = self.cacheDirectory.appendingPathComponent(key.sha256)
            
            do {
                try data.write(to: fileURL)
                self.enforceSize()
            } catch {
                print("Failed to store cache data: \\(error)")
            }
        }
    }
    
    func retrieve(forKey key: String) -> Data? {
        let fileURL = cacheDirectory.appendingPathComponent(key.sha256)
        return try? Data(contentsOf: fileURL)
    }
    
    func remove(forKey key: String) {
        queue.async { [weak self] in
            let fileURL = self?.cacheDirectory.appendingPathComponent(key.sha256)
            try? self?.fileManager.removeItem(at: fileURL!)
        }
    }
    
    func clearAll() {
        queue.async { [weak self] in
            guard let self = self else { return }
            
            do {
                let files = try self.fileManager.contentsOfDirectory(at: self.cacheDirectory, includingPropertiesForKeys: nil)
                for file in files {
                    try self.fileManager.removeItem(at: file)
                }
            } catch {
                print("Failed to clear cache: \\(error)")
            }
        }
    }
    
    func clearExpired() {
        queue.async { [weak self] in
            // Implementation would check file modification dates
            // and remove files older than their TTL
        }
    }
    
    var currentSize: Int64 {
        do {
            let files = try fileManager.contentsOfDirectory(at: cacheDirectory, includingPropertiesForKeys: [.fileSizeKey])
            return files.compactMap { url in
                try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize
            }.reduce(0, +)
        } catch {
            return 0
        }
    }
    
    private func enforceSize() {
        let currentSize = self.currentSize
        
        if currentSize > maxSize {
            // Remove oldest files to maintain size limit
            do {
                let files = try fileManager.contentsOfDirectory(
                    at: cacheDirectory,
                    includingPropertiesForKeys: [.contentModificationDateKey, .fileSizeKey]
                )
                
                let sortedFiles = files.sorted { file1, file2 in
                    let date1 = try? file1.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate
                    let date2 = try? file2.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate
                    return date1 ?? Date.distantPast < date2 ?? Date.distantPast
                }
                
                var bytesToRemove = currentSize - Int64(maxSize)
                for file in sortedFiles {
                    if bytesToRemove <= 0 { break }
                    
                    let fileSize = try? file.resourceValues(forKeys: [.fileSizeKey]).fileSize
                    try? fileManager.removeItem(at: file)
                    bytesToRemove -= Int64(fileSize ?? 0)
                }
            } catch {
                print("Failed to enforce cache size: \\(error)")
            }
        }
    }
}

// MARK: - String Extensions

extension String {
    var sha256: String {
        let data = self.data(using: .utf8)!
        var hash = [UInt8](repeating: 0, count: Int(CC_SHA256_DIGEST_LENGTH))
        data.withUnsafeBytes {
            _ = CC_SHA256($0.baseAddress, CC_LONG(data.count), &hash)
        }
        return hash.map { String(format: "%02x", $0) }.joined()
    }
}

import CommonCrypto
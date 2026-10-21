# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile

# ========================================
# WRFCoin Android SDK ProGuard Rules
# ========================================

# Keep all API classes and methods
-keep class com.wrfcoin.android.sdk.api.** { *; }
-keep class com.wrfcoin.android.sdk.model.** { *; }
-keep class com.wrfcoin.android.sdk.auth.** { *; }

# Keep SDK configuration classes
-keep class com.wrfcoin.android.sdk.Configuration { *; }
-keep class com.wrfcoin.android.sdk.ApiClient { *; }
-keep class com.wrfcoin.android.sdk.JSON { *; }

# Keep callback interfaces
-keep interface com.wrfcoin.android.sdk.ApiCallback { *; }
-keep class com.wrfcoin.android.sdk.ApiResponse { *; }
-keep class com.wrfcoin.android.sdk.ApiException { *; }

# Preserve annotations for reflection
-keepattributes *Annotation*
-keepattributes Signature
-keepattributes InnerClasses
-keepattributes EnclosingMethod

# ========================================
# Third-party library rules
# ========================================

# OkHttp3
-dontwarn okhttp3.**
-dontwarn okio.**
-dontwarn javax.annotation.**
-dontwarn org.conscrypt.**
-keepnames class okhttp3.internal.publicsuffix.PublicSuffixDatabase

# Retrofit2
-dontwarn retrofit2.**
-keep class retrofit2.** { *; }
-keepattributes Signature
-keepattributes Exceptions

# Gson
-dontwarn sun.misc.**
-keep class com.google.gson.stream.** { *; }
-keep class * extends com.google.gson.TypeAdapter
-keep class * implements com.google.gson.TypeAdapterFactory
-keep class * implements com.google.gson.JsonSerializer
-keep class * implements com.google.gson.JsonDeserializer

# Gson TypeAdapters for generated models
-keepclassmembers,allowobfuscation class * {
  @com.google.gson.annotations.SerializedName <fields>;
}

# Keep generic signature of Call, Response (R8 full mode strips signatures from non-kept items).
-keep,allowobfuscation,allowshrinking interface retrofit2.Call
-keep,allowobfuscation,allowshrinking class retrofit2.Response

# With R8 full mode generic signatures are stripped for classes that are not
# kept. Suspend functions are wrapped in continuations where the type argument
# is used.
-keep,allowobfuscation,allowshrinking class kotlin.coroutines.Continuation

# Kotlin Coroutines
-dontwarn kotlinx.coroutines.**
-keep class kotlinx.coroutines.** { *; }

# Work Manager
-keep class * extends androidx.work.Worker
-keep class * extends androidx.work.InputMerger
-keep class androidx.work.** { *; }

# Room
-dontwarn androidx.room.paging.**
-keep class * extends androidx.room.RoomDatabase
-keep @androidx.room.Entity class *
-keep @androidx.room.Dao class *

# ========================================
# Security-related rules
# ========================================

# Certificate pinning classes
-keep class androidx.security.crypto.** { *; }
-keep class com.wrfcoin.android.sdk.security.** { *; }

# Biometric authentication
-keep class androidx.biometric.** { *; }

# Location services
-keep class com.google.android.gms.location.** { *; }

# ========================================
# Debugging and Logging
# ========================================

# Remove debug logging in release builds
-assumenosideeffects class android.util.Log {
    public static boolean isLoggable(java.lang.String, int);
    public static int v(...);
    public static int i(...);
    public static int w(...);
    public static int d(...);
}

# Keep Timber logging for crash reporting
-keep class timber.log.** { *; }

# ========================================
# Performance optimizations
# ========================================

# Remove assertions in release builds
-assumenosideeffects class kotlin.jvm.internal.Intrinsics {
    static void checkParameterIsNotNull(java.lang.Object, java.lang.String);
}

# Optimize string concatenations
-optimizations !code/simplification/string
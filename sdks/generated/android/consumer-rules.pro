# Consumer ProGuard rules for WRFCoin Android SDK
# These rules will be applied to the consumer application when the SDK is used

# Keep all public API classes that consumers will use
-keep public class com.wrfcoin.android.sdk.** { 
    public *; 
}

# Keep model classes for serialization
-keep class com.wrfcoin.android.sdk.model.** { *; }

# Keep API interfaces
-keep interface com.wrfcoin.android.sdk.api.** { *; }

# Keep callback and response classes
-keep class com.wrfcoin.android.sdk.ApiCallback { *; }
-keep class com.wrfcoin.android.sdk.ApiResponse { *; }
-keep class com.wrfcoin.android.sdk.ApiException { *; }

# Preserve enum classes
-keepclassmembers enum * {
    public static **[] values();
    public static ** valueOf(java.lang.String);
}

# Keep Parcelable implementations
-keep class * implements android.os.Parcelable {
    public static final ** CREATOR;
}
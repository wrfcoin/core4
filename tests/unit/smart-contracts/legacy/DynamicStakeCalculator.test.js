const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('DynamicStakeCalculator Library Tests', function () {
  let StakeCalculatorTest;
  let stakeCalculator;
  let owner, participant1, participant2, emergencyMultisig, treasury;

  // Test constants matching library values
  const PRECISION = ethers.parseUnits('1', 6); // 1e6
  const BASE_STAKE_USD = ethers.parseUnits('50', 6); // $50 with 6 decimals precision
  const MIN_STAKE_FLOOR_USD = ethers.parseUnits('20', 6); // $20 with 6 decimals precision
  const MAX_SOCIAL_DISCOUNT = ethers.parseUnits('0.2', 6); // 20% with precision

  // Participation types enum values
  const ParticipationType = {
    WEATHER_STATION: 0,
    FORECAST_PRODUCER: 1,
    DATA_STORAGE_NODE: 2,
    VALIDATOR_NODE: 3,
    HYBRID_NODE: 4,
  };

  // Geographic zones enum values
  const GeographicZone = {
    DEFAULT_AREA: 0,
    UNDERSERVED_AREA: 1,
    DENSE_URBAN_AREA: 2,
    RURAL_REMOTE_AREA: 3,
    ISLAND_COASTAL_ZONE: 4,
  };

  // Verification levels enum values
  const VerificationLevel = {
    UNVERIFIED: 0,
    EMAIL_VERIFIED: 1,
    PHONE_VERIFIED: 2,
    GOVERNMENT_ID_VERIFIED: 3,
    PHYSICAL_STATION_INSPECTED: 4,
  };

  beforeEach(async function () {
    [owner, participant1, participant2, emergencyMultisig, treasury] =
      await ethers.getSigners();

    // Deploy test contract that uses the DynamicStakeCalculator library
    const StakeCalculatorTestFactory = await ethers.getContractFactory(
      'DynamicStakeCalculatorTest'
    );
    stakeCalculator = await StakeCalculatorTestFactory.deploy();
    await stakeCalculator.waitForDeployment();
  });

  describe('Library Constants', function () {
    it('Should return correct precision value', async function () {
      const precision = await stakeCalculator.getPrecision();
      expect(precision).to.equal(PRECISION);
    });

    it('Should return correct base stake amount', async function () {
      const baseStake = await stakeCalculator.getBaseStake();
      expect(baseStake).to.equal(BASE_STAKE_USD);
    });

    it('Should return correct minimum floor', async function () {
      const minFloor = await stakeCalculator.getMinimumFloor();
      expect(minFloor).to.equal(MIN_STAKE_FLOOR_USD);
    });

    it('Should return correct maximum social discount', async function () {
      const maxDiscount = await stakeCalculator.getMaxSocialDiscount();
      expect(maxDiscount).to.equal(MAX_SOCIAL_DISCOUNT);
    });
  });

  describe('Participation Type Multipliers', function () {
    it('Should return 1.0x multiplier for Weather Station', async function () {
      const hybridServices = {
        providesWeatherData: false,
        providesForecasting: false,
        providesDataStorage: false,
        providesValidation: false,
      };

      const multiplier = await stakeCalculator.getParticipationMultiplier(
        ParticipationType.WEATHER_STATION,
        hybridServices
      );

      expect(multiplier).to.equal(PRECISION); // 1.0x
    });

    it('Should return 0.8x multiplier for Forecast Producer', async function () {
      const hybridServices = {
        providesWeatherData: false,
        providesForecasting: false,
        providesDataStorage: false,
        providesValidation: false,
      };

      const multiplier = await stakeCalculator.getParticipationMultiplier(
        ParticipationType.FORECAST_PRODUCER,
        hybridServices
      );

      expect(multiplier).to.equal(ethers.parseUnits('0.8', 6)); // 0.8x
    });

    it('Should return 0.6x multiplier for Data Storage Node', async function () {
      const hybridServices = {
        providesWeatherData: false,
        providesForecasting: false,
        providesDataStorage: false,
        providesValidation: false,
      };

      const multiplier = await stakeCalculator.getParticipationMultiplier(
        ParticipationType.DATA_STORAGE_NODE,
        hybridServices
      );

      expect(multiplier).to.equal(ethers.parseUnits('0.6', 6)); // 0.6x
    });

    it('Should return 1.2x multiplier for Validator Node', async function () {
      const hybridServices = {
        providesWeatherData: false,
        providesForecasting: false,
        providesDataStorage: false,
        providesValidation: false,
      };

      const multiplier = await stakeCalculator.getParticipationMultiplier(
        ParticipationType.VALIDATOR_NODE,
        hybridServices
      );

      expect(multiplier).to.equal(ethers.parseUnits('1.2', 6)); // 1.2x
    });

    it('Should calculate correct hybrid multiplier with multiple services', async function () {
      const hybridServices = {
        providesWeatherData: true,
        providesForecasting: true,
        providesDataStorage: true,
        providesValidation: true,
      };

      const multiplier = await stakeCalculator.getParticipationMultiplier(
        ParticipationType.HYBRID_NODE,
        hybridServices
      );

      // Should start at 1.2x and get 15% discount (3 additional services * 5%)
      const expected = ethers.parseUnits('1.05', 6); // 1.2 - 0.15 = 1.05x
      expect(multiplier).to.equal(expected);
    });
  });

  describe('Geographic Zone Multipliers', function () {
    it('Should return 1.0x for default area', async function () {
      const multiplier = await stakeCalculator.getGeographicMultiplier(
        GeographicZone.DEFAULT_AREA
      );
      expect(multiplier).to.equal(PRECISION);
    });

    it('Should return 0.3x for underserved area (high incentive)', async function () {
      const multiplier = await stakeCalculator.getGeographicMultiplier(
        GeographicZone.UNDERSERVED_AREA
      );
      expect(multiplier).to.equal(ethers.parseUnits('0.3', 6));
    });

    it('Should return 2.0x for dense urban area (high competition)', async function () {
      const multiplier = await stakeCalculator.getGeographicMultiplier(
        GeographicZone.DENSE_URBAN_AREA
      );
      expect(multiplier).to.equal(ethers.parseUnits('2.0', 6));
    });

    it('Should return 0.5x for rural/remote area', async function () {
      const multiplier = await stakeCalculator.getGeographicMultiplier(
        GeographicZone.RURAL_REMOTE_AREA
      );
      expect(multiplier).to.equal(ethers.parseUnits('0.5', 6));
    });

    it('Should return 0.4x for island/coastal zone', async function () {
      const multiplier = await stakeCalculator.getGeographicMultiplier(
        GeographicZone.ISLAND_COASTAL_ZONE
      );
      expect(multiplier).to.equal(ethers.parseUnits('0.4', 6));
    });
  });

  describe('Network Density Multipliers', function () {
    it('Should return 0.8x for no nearby stations', async function () {
      const density = {
        nearbyStationsCount: 0,
        isDensityVerified: true,
      };

      const multiplier =
        await stakeCalculator.getNetworkDensityMultiplier(density);
      expect(multiplier).to.equal(ethers.parseUnits('0.8', 6));
    });

    it('Should return 1.0x for 1-3 nearby stations', async function () {
      const density = {
        nearbyStationsCount: 2,
        isDensityVerified: true,
      };

      const multiplier =
        await stakeCalculator.getNetworkDensityMultiplier(density);
      expect(multiplier).to.equal(PRECISION);
    });

    it('Should return 1.5x for 4-10 nearby stations', async function () {
      const density = {
        nearbyStationsCount: 7,
        isDensityVerified: true,
      };

      const multiplier =
        await stakeCalculator.getNetworkDensityMultiplier(density);
      expect(multiplier).to.equal(ethers.parseUnits('1.5', 6));
    });

    it('Should return 2.0x for >10 nearby stations', async function () {
      const density = {
        nearbyStationsCount: 15,
        isDensityVerified: true,
      };

      const multiplier =
        await stakeCalculator.getNetworkDensityMultiplier(density);
      expect(multiplier).to.equal(ethers.parseUnits('2.0', 6));
    });
  });

  describe('Verification Level Discounts', function () {
    it('Should return 1.0x for unverified', async function () {
      const discount = await stakeCalculator.getVerificationDiscount(
        VerificationLevel.UNVERIFIED
      );
      expect(discount).to.equal(PRECISION);
    });

    it('Should return 0.95x for email verified', async function () {
      const discount = await stakeCalculator.getVerificationDiscount(
        VerificationLevel.EMAIL_VERIFIED
      );
      expect(discount).to.equal(ethers.parseUnits('0.95', 6));
    });

    it('Should return 0.90x for phone verified', async function () {
      const discount = await stakeCalculator.getVerificationDiscount(
        VerificationLevel.PHONE_VERIFIED
      );
      expect(discount).to.equal(ethers.parseUnits('0.90', 6));
    });

    it('Should return 0.80x for government ID verified', async function () {
      const discount = await stakeCalculator.getVerificationDiscount(
        VerificationLevel.GOVERNMENT_ID_VERIFIED
      );
      expect(discount).to.equal(ethers.parseUnits('0.80', 6));
    });

    it('Should return 0.70x for physical station inspected', async function () {
      const discount = await stakeCalculator.getVerificationDiscount(
        VerificationLevel.PHYSICAL_STATION_INSPECTED
      );
      expect(discount).to.equal(ethers.parseUnits('0.70', 6));
    });
  });

  describe('Social Verification Discounts', function () {
    it('Should return 1.0x for no social verification', async function () {
      const social = {
        githubVerified: false,
        twitterVerified: false,
        linkedinVerified: false,
      };

      const discount = await stakeCalculator.getSocialDiscount(social);
      expect(discount).to.equal(PRECISION);
    });

    it('Should return 0.90x for GitHub verification only', async function () {
      const social = {
        githubVerified: true,
        twitterVerified: false,
        linkedinVerified: false,
      };

      const discount = await stakeCalculator.getSocialDiscount(social);
      expect(discount).to.equal(ethers.parseUnits('0.90', 6)); // 10% discount
    });

    it('Should apply additive discounts up to 20% maximum', async function () {
      const social = {
        githubVerified: true, // 10%
        twitterVerified: true, // 5%
        linkedinVerified: true, // 7%
      };

      const discount = await stakeCalculator.getSocialDiscount(social);
      // Total would be 22% but capped at 20%
      expect(discount).to.equal(ethers.parseUnits('0.80', 6)); // 20% discount maximum
    });

    it('Should handle partial social verification', async function () {
      const social = {
        githubVerified: true, // 10%
        twitterVerified: false,
        linkedinVerified: true, // 7%
      };

      const discount = await stakeCalculator.getSocialDiscount(social);
      expect(discount).to.equal(ethers.parseUnits('0.83', 6)); // 17% discount
    });
  });

  describe('Complete Stake Calculations', function () {
    it('Should calculate minimum stake for highly incentivized setup', async function () {
      // Underserved area, data storage node, fully verified, all social verification
      const params = {
        participationType: ParticipationType.DATA_STORAGE_NODE,
        geographicZone: GeographicZone.UNDERSERVED_AREA,
        verificationLevel: VerificationLevel.PHYSICAL_STATION_INSPECTED,
        social: {
          githubVerified: true,
          twitterVerified: true,
          linkedinVerified: true,
        },
        density: {
          nearbyStationsCount: 0, // No competition
          isDensityVerified: true,
        },
        hybridServices: {
          providesWeatherData: false,
          providesForecasting: false,
          providesDataStorage: false,
          providesValidation: false,
        },
      };

      const result = await stakeCalculator.calculateRequiredStake(params);

      // Base: $50 * 0.6 (storage) * 0.3 (underserved) * 0.8 (no competition) * 0.7 (verified) * 0.8 (social)
      // = $50 * 0.6 * 0.3 * 0.8 * 0.7 * 0.8 = $4.032
      // But should hit $20 minimum floor
      expect(result.finalStakeUSD).to.equal(MIN_STAKE_FLOOR_USD);
      expect(result.hitMinimumFloor).to.be.true;
    });

    it('Should calculate maximum stake for high-competition setup', async function () {
      const params = {
        participationType: ParticipationType.VALIDATOR_NODE,
        geographicZone: GeographicZone.DENSE_URBAN_AREA,
        verificationLevel: VerificationLevel.UNVERIFIED,
        social: {
          githubVerified: false,
          twitterVerified: false,
          linkedinVerified: false,
        },
        density: {
          nearbyStationsCount: 20, // High competition
          isDensityVerified: true,
        },
        hybridServices: {
          providesWeatherData: false,
          providesForecasting: false,
          providesDataStorage: false,
          providesValidation: false,
        },
      };

      const result = await stakeCalculator.calculateRequiredStake(params);

      // Base: $50 * 1.2 (validator) * 2.0 (urban) * 2.0 (high density) * 1.0 (unverified) * 1.0 (no social)
      // = $50 * 1.2 * 2.0 * 2.0 * 1.0 * 1.0 = $240
      const expected = ethers.parseUnits('240', 6);
      expect(result.finalStakeUSD).to.equal(expected);
      expect(result.hitMinimumFloor).to.be.false;
    });

    it('Should calculate basic weather station stake', async function () {
      const finalStake = await stakeCalculator.calculateBasicStake(
        ParticipationType.WEATHER_STATION,
        GeographicZone.DEFAULT_AREA,
        VerificationLevel.EMAIL_VERIFIED
      );

      // Base: $50 * 1.0 (weather) * 1.0 (default) * 1.0 (normal density) * 0.95 (email) * 1.0 (no social)
      // = $50 * 1.0 * 1.0 * 1.0 * 0.95 * 1.0 = $47.5
      const expected = ethers.parseUnits('47.5', 6);
      expect(finalStake).to.equal(expected);
    });

    it('Should validate calculation precision and consistency', async function () {
      const params = {
        participationType: ParticipationType.FORECAST_PRODUCER,
        geographicZone: GeographicZone.RURAL_REMOTE_AREA,
        verificationLevel: VerificationLevel.PHONE_VERIFIED,
        social: {
          githubVerified: true,
          twitterVerified: false,
          linkedinVerified: false,
        },
        density: {
          nearbyStationsCount: 5,
          isDensityVerified: true,
        },
        hybridServices: {
          providesWeatherData: false,
          providesForecasting: false,
          providesDataStorage: false,
          providesValidation: false,
        },
      };

      const result = await stakeCalculator.calculateRequiredStake(params);

      // Verify all intermediate calculations
      expect(result.baseStakeUSD).to.equal(BASE_STAKE_USD);
      expect(result.participationMultiplier).to.equal(
        ethers.parseUnits('0.8', 6)
      );
      expect(result.geographicMultiplier).to.equal(ethers.parseUnits('0.5', 6));
      expect(result.networkDensityMultiplier).to.equal(
        ethers.parseUnits('1.5', 6)
      );
      expect(result.verificationDiscount).to.equal(
        ethers.parseUnits('0.90', 6)
      );
      expect(result.socialDiscount).to.equal(ethers.parseUnits('0.90', 6));

      // Raw: $50 * 0.8 * 0.5 * 1.5 = $30
      expect(result.rawStakeUSD).to.equal(ethers.parseUnits('30', 6));

      // Discounted: $30 * 0.90 * 0.90 = $24.30
      expect(result.discountedStakeUSD).to.equal(ethers.parseUnits('24.3', 6));
      expect(result.finalStakeUSD).to.equal(ethers.parseUnits('24.3', 6));
      expect(result.hitMinimumFloor).to.be.false;
    });
  });

  describe('Asset Conversion Functions', function () {
    it('Should correctly convert USD to asset amounts', async function () {
      const usdAmount = ethers.parseUnits('100', 6); // $100
      const assetPrice = ethers.parseUnits('2000', 6); // $2000/token
      const decimals = 18;

      const assetAmount = await stakeCalculator.convertToAsset(
        usdAmount,
        assetPrice,
        decimals
      );

      // $100 / $2000 = 0.05 tokens = 5e16 wei
      const expected = ethers.parseUnits('0.05', 18);
      expect(assetAmount).to.equal(expected);
    });

    it('Should handle different asset decimals correctly', async function () {
      const usdAmount = ethers.parseUnits('100', 6); // $100
      const assetPrice = ethers.parseUnits('1', 6); // $1/token (stablecoin)
      const decimals = 6;

      const assetAmount = await stakeCalculator.convertToAsset(
        usdAmount,
        assetPrice,
        decimals
      );

      // $100 / $1 = 100 tokens = 100e6 units
      const expected = ethers.parseUnits('100', 6);
      expect(assetAmount).to.equal(expected);
    });

    it('Should revert on zero asset price', async function () {
      const usdAmount = ethers.parseUnits('100', 6);
      const assetPrice = 0;
      const decimals = 18;

      await expect(
        stakeCalculator.convertToAsset(usdAmount, assetPrice, decimals)
      ).to.be.revertedWith('InvalidAssetPrice');
    });
  });

  describe('Parameter Validation', function () {
    it('Should validate correct parameters', async function () {
      const params = {
        participationType: ParticipationType.WEATHER_STATION,
        geographicZone: GeographicZone.DEFAULT_AREA,
        verificationLevel: VerificationLevel.EMAIL_VERIFIED,
        social: {
          githubVerified: true,
          twitterVerified: false,
          linkedinVerified: false,
        },
        density: {
          nearbyStationsCount: 5,
          isDensityVerified: true,
        },
        hybridServices: {
          providesWeatherData: false,
          providesForecasting: false,
          providesDataStorage: false,
          providesValidation: false,
        },
      };

      const [isValid, errorMessage] =
        await stakeCalculator.validateStakeParameters(params);
      expect(isValid).to.be.true;
      expect(errorMessage).to.equal('');
    });

    it('Should reject invalid participation type', async function () {
      const params = {
        participationType: 99, // Invalid
        geographicZone: GeographicZone.DEFAULT_AREA,
        verificationLevel: VerificationLevel.EMAIL_VERIFIED,
        social: {
          githubVerified: false,
          twitterVerified: false,
          linkedinVerified: false,
        },
        density: {
          nearbyStationsCount: 5,
          isDensityVerified: true,
        },
        hybridServices: {
          providesWeatherData: false,
          providesForecasting: false,
          providesDataStorage: false,
          providesValidation: false,
        },
      };

      const [isValid, errorMessage] =
        await stakeCalculator.validateStakeParameters(params);
      expect(isValid).to.be.false;
      expect(errorMessage).to.equal('Invalid participation type');
    });

    it('Should reject excessive network density count', async function () {
      const params = {
        participationType: ParticipationType.WEATHER_STATION,
        geographicZone: GeographicZone.DEFAULT_AREA,
        verificationLevel: VerificationLevel.EMAIL_VERIFIED,
        social: {
          githubVerified: false,
          twitterVerified: false,
          linkedinVerified: false,
        },
        density: {
          nearbyStationsCount: 1500, // Too high
          isDensityVerified: true,
        },
        hybridServices: {
          providesWeatherData: false,
          providesForecasting: false,
          providesDataStorage: false,
          providesValidation: false,
        },
      };

      const [isValid, errorMessage] =
        await stakeCalculator.validateStakeParameters(params);
      expect(isValid).to.be.false;
      expect(errorMessage).to.equal('Invalid network density count');
    });
  });

  describe('Stake Security Validation', function () {
    it('Should reject stakes below minimum floor', async function () {
      const lowStake = ethers.parseUnits('10', 6); // $10

      const [meetsRequirements, securityLevel] =
        await stakeCalculator.validateStakeSecurity(lowStake);
      expect(meetsRequirements).to.be.false;
      expect(securityLevel).to.equal('Below minimum floor');
    });

    it('Should classify basic security level', async function () {
      const basicStake = ethers.parseUnits('30', 6); // $30

      const [meetsRequirements, securityLevel] =
        await stakeCalculator.validateStakeSecurity(basicStake);
      expect(meetsRequirements).to.be.true;
      expect(securityLevel).to.equal('Basic security');
    });

    it('Should classify enhanced security level', async function () {
      const enhancedStake = ethers.parseUnits('75', 6); // $75

      const [meetsRequirements, securityLevel] =
        await stakeCalculator.validateStakeSecurity(enhancedStake);
      expect(meetsRequirements).to.be.true;
      expect(securityLevel).to.equal('Enhanced security');
    });

    it('Should classify maximum security level', async function () {
      const maxStake = ethers.parseUnits('150', 6); // $150

      const [meetsRequirements, securityLevel] =
        await stakeCalculator.validateStakeSecurity(maxStake);
      expect(meetsRequirements).to.be.true;
      expect(securityLevel).to.equal('Maximum security');
    });
  });

  describe('Edge Cases and Gas Optimization', function () {
    it('Should handle zero network density gracefully', async function () {
      const density = {
        nearbyStationsCount: 0,
        isDensityVerified: false,
      };

      const multiplier =
        await stakeCalculator.getNetworkDensityMultiplier(density);
      expect(multiplier).to.equal(ethers.parseUnits('0.8', 6));
    });

    it('Should handle all hybrid services enabled', async function () {
      const hybridServices = {
        providesWeatherData: true,
        providesForecasting: true,
        providesDataStorage: true,
        providesValidation: true,
      };

      const multiplier = await stakeCalculator.getParticipationMultiplier(
        ParticipationType.HYBRID_NODE,
        hybridServices
      );

      // Should get maximum 15% discount from base 1.2x
      const expected = ethers.parseUnits('1.05', 6);
      expect(multiplier).to.equal(expected);
    });

    it('Should maintain precision in complex calculations', async function () {
      // Test with fractional results that might cause precision issues
      const params = {
        participationType: ParticipationType.DATA_STORAGE_NODE, // 0.6x
        geographicZone: GeographicZone.ISLAND_COASTAL_ZONE, // 0.4x
        verificationLevel: VerificationLevel.GOVERNMENT_ID_VERIFIED, // 0.8x
        social: {
          githubVerified: true, // Combined social should be ~0.83x
          twitterVerified: false,
          linkedinVerified: true,
        },
        density: {
          nearbyStationsCount: 7, // 1.5x
          isDensityVerified: true,
        },
        hybridServices: {
          providesWeatherData: false,
          providesForecasting: false,
          providesDataStorage: false,
          providesValidation: false,
        },
      };

      const result = await stakeCalculator.calculateRequiredStake(params);

      // Even with fractional multipliers, should calculate correctly
      expect(result.finalStakeUSD).to.equal(MIN_STAKE_FLOOR_USD); // Should hit floor
      expect(result.hitMinimumFloor).to.be.true;
    });
  });
});

// Helper test contract that exposes library functions for testing
const testContractSource = `
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../contracts/libraries/DynamicStakeCalculator.sol";

contract DynamicStakeCalculatorTest {
    using DynamicStakeCalculator for DynamicStakeCalculator.StakeParameters;
    
    function getPrecision() external pure returns (uint256) {
        return DynamicStakeCalculator.getPrecision();
    }
    
    function getBaseStake() external pure returns (uint256) {
        return DynamicStakeCalculator.getBaseStake();
    }
    
    function getMinimumFloor() external pure returns (uint256) {
        return DynamicStakeCalculator.getMinimumFloor();
    }
    
    function getMaxSocialDiscount() external pure returns (uint256) {
        return DynamicStakeCalculator.getMaxSocialDiscount();
    }
    
    function getParticipationMultiplier(
        DynamicStakeCalculator.ParticipationType participationType,
        DynamicStakeCalculator.HybridServices memory hybridServices
    ) external pure returns (uint256) {
        return DynamicStakeCalculator.getParticipationMultiplier(participationType, hybridServices);
    }
    
    function getGeographicMultiplier(
        DynamicStakeCalculator.GeographicZone zone
    ) external pure returns (uint256) {
        return DynamicStakeCalculator.getGeographicMultiplier(zone);
    }
    
    function getNetworkDensityMultiplier(
        DynamicStakeCalculator.NetworkDensity memory density
    ) external pure returns (uint256) {
        return DynamicStakeCalculator.getNetworkDensityMultiplier(density);
    }
    
    function getVerificationDiscount(
        DynamicStakeCalculator.VerificationLevel level
    ) external pure returns (uint256) {
        return DynamicStakeCalculator.getVerificationDiscount(level);
    }
    
    function getSocialDiscount(
        DynamicStakeCalculator.SocialVerification memory social
    ) external pure returns (uint256) {
        return DynamicStakeCalculator.getSocialDiscount(social);
    }
    
    function calculateRequiredStake(
        DynamicStakeCalculator.StakeParameters memory params
    ) external pure returns (DynamicStakeCalculator.StakeCalculationResult memory) {
        return DynamicStakeCalculator.calculateRequiredStake(params);
    }
    
    function calculateBasicStake(
        DynamicStakeCalculator.ParticipationType participationType,
        DynamicStakeCalculator.GeographicZone geographicZone,
        DynamicStakeCalculator.VerificationLevel verificationLevel
    ) external pure returns (uint256) {
        return DynamicStakeCalculator.calculateBasicStake(participationType, geographicZone, verificationLevel);
    }
    
    function convertToAsset(
        uint256 usdAmount,
        uint256 assetPriceUSD,
        uint8 assetDecimals
    ) external pure returns (uint256) {
        return DynamicStakeCalculator.convertToAsset(usdAmount, assetPriceUSD, assetDecimals);
    }
    
    function validateStakeParameters(
        DynamicStakeCalculator.StakeParameters memory params
    ) external pure returns (bool, string memory) {
        return DynamicStakeCalculator.validateStakeParameters(params);
    }
    
    function validateStakeSecurity(
        uint256 stakeUSD
    ) external pure returns (bool, string memory) {
        return DynamicStakeCalculator.validateStakeSecurity(stakeUSD);
    }
}
`;

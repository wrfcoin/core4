/**
 * Carbon Credit Trading Tests
 * Task 114.2: Order management, MEV protection, price matching, settlement
 *
 * Tests marketplace operations including order book management,
 * trade execution, and MEV protection mechanisms
 * Target: 15 tests
 */

import {
  describe,
  it,
  expect,
  beforeEach,
  afterEach,
  jest,
} from '@jest/globals';

// Types based on carbon-marketplace-service.ts
enum OrderType {
  BUY = 'BUY',
  SELL = 'SELL',
}

enum OrderStatus {
  PENDING_COMMIT = 'PENDING_COMMIT',
  COMMITTED = 'COMMITTED',
  ACTIVE = 'ACTIVE',
  PARTIALLY_FILLED = 'PARTIALLY_FILLED',
  FILLED = 'FILLED',
  CANCELLED = 'CANCELLED',
  EXPIRED = 'EXPIRED',
}

enum CreditType {
  VCS = 'VCS',
  GOLD_STANDARD = 'GOLD_STANDARD',
  CDM = 'CDM',
  VERRA = 'VERRA',
  ACR = 'ACR',
}

interface CarbonCreditOrder {
  id: string;
  userId: string;
  walletAddress: string;
  orderType: OrderType;
  creditType: CreditType;
  quantity: bigint;
  price: bigint;
  remainingQuantity: bigint;
  status: OrderStatus;
  commitHash?: string;
  revealDeadline?: Date;
  createdAt: Date;
  updatedAt: Date;
  expiresAt?: Date;
}

interface Trade {
  id: string;
  buyOrderId: string;
  sellOrderId: string;
  buyerId: string;
  sellerId: string;
  creditType: CreditType;
  quantity: bigint;
  price: bigint;
  totalValue: bigint;
  fee: bigint;
  status: 'PENDING' | 'EXECUTED' | 'SETTLED' | 'FAILED';
  executedAt: Date;
}

// Mock Marketplace Service
class MockCarbonMarketplaceService {
  private orders: Map<string, CarbonCreditOrder> = new Map();
  private commits: Map<
    string,
    { commitHash: string; userId: string; revealDeadline: Date }
  > = new Map();
  private trades: Trade[] = [];
  private orderCounter = 0;

  private readonly FEE_BASIS_POINTS = 30n; // 0.3%
  private readonly COMMIT_REVEAL_WINDOW = 2 * 60 * 1000; // 2 minutes
  private readonly MIN_ORDER_SIZE = 1000000000000000n; // 0.001 credits

  // Create a commit for MEV protection
  async createCommit(userId: string, orderDetails: string): Promise<string> {
    const commitHash = this.generateHash(orderDetails + Date.now().toString());
    const revealDeadline = new Date(Date.now() + this.COMMIT_REVEAL_WINDOW);

    this.commits.set(commitHash, {
      commitHash,
      userId,
      revealDeadline,
    });

    return commitHash;
  }

  // Reveal and create order (MEV protection)
  async revealAndCreateOrder(
    commitHash: string,
    userId: string,
    orderType: OrderType,
    creditType: CreditType,
    quantity: bigint,
    price: bigint
  ): Promise<CarbonCreditOrder> {
    const commit = this.commits.get(commitHash);

    if (!commit) {
      throw new Error('Invalid commit hash');
    }

    if (commit.userId !== userId) {
      throw new Error('User mismatch for commit');
    }

    if (new Date() > commit.revealDeadline) {
      throw new Error('Commit reveal deadline expired');
    }

    if (quantity < this.MIN_ORDER_SIZE) {
      throw new Error('Order quantity below minimum');
    }

    const order: CarbonCreditOrder = {
      id: `order_${++this.orderCounter}`,
      userId,
      walletAddress: `0x${userId.slice(0, 40)}`,
      orderType,
      creditType,
      quantity,
      price,
      remainingQuantity: quantity,
      status: OrderStatus.ACTIVE,
      commitHash,
      createdAt: new Date(),
      updatedAt: new Date(),
    };

    this.orders.set(order.id, order);
    this.commits.delete(commitHash);

    return order;
  }

  // Create order without MEV protection (for testing)
  async createOrder(
    userId: string,
    orderType: OrderType,
    creditType: CreditType,
    quantity: bigint,
    price: bigint
  ): Promise<CarbonCreditOrder> {
    if (quantity < this.MIN_ORDER_SIZE) {
      throw new Error('Order quantity below minimum');
    }

    const order: CarbonCreditOrder = {
      id: `order_${++this.orderCounter}`,
      userId,
      walletAddress: `0x${userId.slice(0, 40)}`,
      orderType,
      creditType,
      quantity,
      price,
      remainingQuantity: quantity,
      status: OrderStatus.ACTIVE,
      createdAt: new Date(),
      updatedAt: new Date(),
    };

    this.orders.set(order.id, order);
    return order;
  }

  // Cancel an order
  async cancelOrder(
    orderId: string,
    userId: string
  ): Promise<CarbonCreditOrder> {
    const order = this.orders.get(orderId);

    if (!order) {
      throw new Error('Order not found');
    }

    if (order.userId !== userId) {
      throw new Error('Not authorized to cancel this order');
    }

    if (
      order.status === OrderStatus.FILLED ||
      order.status === OrderStatus.CANCELLED
    ) {
      throw new Error('Order cannot be cancelled');
    }

    order.status = OrderStatus.CANCELLED;
    order.updatedAt = new Date();

    return order;
  }

  // Match orders (price-time priority)
  async matchOrders(creditType: CreditType): Promise<Trade[]> {
    const buyOrders = Array.from(this.orders.values())
      .filter(
        o =>
          o.orderType === OrderType.BUY &&
          o.creditType === creditType &&
          o.status === OrderStatus.ACTIVE
      )
      .sort(
        (a, b) =>
          Number(b.price - a.price) ||
          a.createdAt.getTime() - b.createdAt.getTime()
      );

    const sellOrders = Array.from(this.orders.values())
      .filter(
        o =>
          o.orderType === OrderType.SELL &&
          o.creditType === creditType &&
          o.status === OrderStatus.ACTIVE
      )
      .sort(
        (a, b) =>
          Number(a.price - b.price) ||
          a.createdAt.getTime() - b.createdAt.getTime()
      );

    const trades: Trade[] = [];

    for (const buyOrder of buyOrders) {
      for (const sellOrder of sellOrders) {
        if (
          buyOrder.price >= sellOrder.price &&
          buyOrder.remainingQuantity > 0n &&
          sellOrder.remainingQuantity > 0n
        ) {
          const matchQuantity =
            buyOrder.remainingQuantity < sellOrder.remainingQuantity
              ? buyOrder.remainingQuantity
              : sellOrder.remainingQuantity;

          const matchPrice = sellOrder.price; // Price-time priority uses maker's price
          const totalValue = matchQuantity * matchPrice;
          const fee = (totalValue * this.FEE_BASIS_POINTS) / 10000n;

          const trade: Trade = {
            id: `trade_${this.trades.length + 1}`,
            buyOrderId: buyOrder.id,
            sellOrderId: sellOrder.id,
            buyerId: buyOrder.userId,
            sellerId: sellOrder.userId,
            creditType,
            quantity: matchQuantity,
            price: matchPrice,
            totalValue,
            fee,
            status: 'EXECUTED',
            executedAt: new Date(),
          };

          // Update order quantities
          buyOrder.remainingQuantity -= matchQuantity;
          sellOrder.remainingQuantity -= matchQuantity;

          // Update order statuses
          buyOrder.status =
            buyOrder.remainingQuantity === 0n
              ? OrderStatus.FILLED
              : OrderStatus.PARTIALLY_FILLED;
          sellOrder.status =
            sellOrder.remainingQuantity === 0n
              ? OrderStatus.FILLED
              : OrderStatus.PARTIALLY_FILLED;

          this.trades.push(trade);
          trades.push(trade);
        }
      }
    }

    return trades;
  }

  // Get order by ID
  getOrder(orderId: string): CarbonCreditOrder | undefined {
    return this.orders.get(orderId);
  }

  // Get all trades
  getTrades(): Trade[] {
    return [...this.trades];
  }

  // Calculate trading fee
  calculateFee(totalValue: bigint): bigint {
    return (totalValue * this.FEE_BASIS_POINTS) / 10000n;
  }

  private generateHash(data: string): string {
    // Simplified hash for testing
    let hash = 0;
    for (let i = 0; i < data.length; i++) {
      hash = (hash << 5) - hash + data.charCodeAt(i);
      hash = hash & hash;
    }
    return `commit_${Math.abs(hash).toString(16)}`;
  }
}

describe('Carbon Credit Trading Service', () => {
  let marketplaceService: MockCarbonMarketplaceService;

  beforeEach(() => {
    marketplaceService = new MockCarbonMarketplaceService();
  });

  describe('Order Management', () => {
    it('should create a buy order', async () => {
      const order = await marketplaceService.createOrder(
        'user123',
        OrderType.BUY,
        CreditType.VCS,
        10000000000000000n, // 0.01 credits
        1000000000000000000n // 1 WRF per credit
      );

      expect(order.id).toBeDefined();
      expect(order.orderType).toBe(OrderType.BUY);
      expect(order.status).toBe(OrderStatus.ACTIVE);
      expect(order.remainingQuantity).toBe(order.quantity);
    });

    it('should create a sell order', async () => {
      const order = await marketplaceService.createOrder(
        'user456',
        OrderType.SELL,
        CreditType.GOLD_STANDARD,
        50000000000000000n, // 0.05 credits
        2000000000000000000n // 2 WRF per credit
      );

      expect(order.id).toBeDefined();
      expect(order.orderType).toBe(OrderType.SELL);
      expect(order.creditType).toBe(CreditType.GOLD_STANDARD);
    });

    it('should reject orders below minimum size', async () => {
      await expect(
        marketplaceService.createOrder(
          'user123',
          OrderType.BUY,
          CreditType.VCS,
          100n, // Too small
          1000000000000000000n
        )
      ).rejects.toThrow('Order quantity below minimum');
    });

    it('should cancel an order', async () => {
      const order = await marketplaceService.createOrder(
        'user123',
        OrderType.BUY,
        CreditType.VCS,
        10000000000000000n,
        1000000000000000000n
      );

      const cancelled = await marketplaceService.cancelOrder(
        order.id,
        'user123'
      );
      expect(cancelled.status).toBe(OrderStatus.CANCELLED);
    });

    it('should not allow unauthorized cancellation', async () => {
      const order = await marketplaceService.createOrder(
        'user123',
        OrderType.BUY,
        CreditType.VCS,
        10000000000000000n,
        1000000000000000000n
      );

      await expect(
        marketplaceService.cancelOrder(order.id, 'user456')
      ).rejects.toThrow('Not authorized');
    });
  });

  describe('MEV Protection (Commit-Reveal)', () => {
    it('should create a commit hash', async () => {
      const commitHash = await marketplaceService.createCommit(
        'user123',
        'buy_vcs_100_1000'
      );
      expect(commitHash).toBeDefined();
      expect(commitHash.startsWith('commit_')).toBe(true);
    });

    it('should reveal and create order with valid commit', async () => {
      const commitHash = await marketplaceService.createCommit(
        'user123',
        'buy_vcs_100_1000'
      );

      const order = await marketplaceService.revealAndCreateOrder(
        commitHash,
        'user123',
        OrderType.BUY,
        CreditType.VCS,
        10000000000000000n,
        1000000000000000000n
      );

      expect(order.status).toBe(OrderStatus.ACTIVE);
      expect(order.commitHash).toBe(commitHash);
    });

    it('should reject reveal with invalid commit hash', async () => {
      await expect(
        marketplaceService.revealAndCreateOrder(
          'invalid_commit',
          'user123',
          OrderType.BUY,
          CreditType.VCS,
          10000000000000000n,
          1000000000000000000n
        )
      ).rejects.toThrow('Invalid commit hash');
    });

    it('should reject reveal with wrong user', async () => {
      const commitHash = await marketplaceService.createCommit(
        'user123',
        'buy_vcs_100_1000'
      );

      await expect(
        marketplaceService.revealAndCreateOrder(
          commitHash,
          'user456', // Wrong user
          OrderType.BUY,
          CreditType.VCS,
          10000000000000000n,
          1000000000000000000n
        )
      ).rejects.toThrow('User mismatch');
    });
  });

  describe('Price Matching', () => {
    it('should match compatible buy and sell orders', async () => {
      // Create sell order at 1 WRF
      await marketplaceService.createOrder(
        'seller1',
        OrderType.SELL,
        CreditType.VCS,
        10000000000000000n,
        1000000000000000000n
      );

      // Create buy order at 1.1 WRF (willing to pay more)
      await marketplaceService.createOrder(
        'buyer1',
        OrderType.BUY,
        CreditType.VCS,
        10000000000000000n,
        1100000000000000000n
      );

      const trades = await marketplaceService.matchOrders(CreditType.VCS);

      expect(trades.length).toBe(1);
      expect(trades[0].buyerId).toBe('buyer1');
      expect(trades[0].sellerId).toBe('seller1');
    });

    it('should not match orders with incompatible prices', async () => {
      // Create sell order at 2 WRF
      await marketplaceService.createOrder(
        'seller1',
        OrderType.SELL,
        CreditType.VCS,
        10000000000000000n,
        2000000000000000000n
      );

      // Create buy order at 1 WRF (not willing to pay more)
      await marketplaceService.createOrder(
        'buyer1',
        OrderType.BUY,
        CreditType.VCS,
        10000000000000000n,
        1000000000000000000n
      );

      const trades = await marketplaceService.matchOrders(CreditType.VCS);

      expect(trades.length).toBe(0);
    });

    it('should partially fill orders when quantities differ', async () => {
      // Sell order for 100 units
      await marketplaceService.createOrder(
        'seller1',
        OrderType.SELL,
        CreditType.VCS,
        100000000000000000n,
        1000000000000000000n
      );

      // Buy order for 40 units
      await marketplaceService.createOrder(
        'buyer1',
        OrderType.BUY,
        CreditType.VCS,
        40000000000000000n,
        1000000000000000000n
      );

      const trades = await marketplaceService.matchOrders(CreditType.VCS);

      expect(trades.length).toBe(1);
      expect(trades[0].quantity).toBe(40000000000000000n);
    });

    it('should use price-time priority for matching', async () => {
      // First seller at 1 WRF
      const firstSeller = await marketplaceService.createOrder(
        'seller1',
        OrderType.SELL,
        CreditType.VCS,
        10000000000000000n,
        1000000000000000000n
      );

      // Second seller at same price (later)
      await new Promise(resolve => setTimeout(resolve, 10));
      await marketplaceService.createOrder(
        'seller2',
        OrderType.SELL,
        CreditType.VCS,
        10000000000000000n,
        1000000000000000000n
      );

      // Buyer at 1.5 WRF
      await marketplaceService.createOrder(
        'buyer1',
        OrderType.BUY,
        CreditType.VCS,
        10000000000000000n,
        1500000000000000000n
      );

      const trades = await marketplaceService.matchOrders(CreditType.VCS);

      // First seller should be matched (time priority)
      expect(trades[0].sellOrderId).toBe(firstSeller.id);
    });
  });

  describe('Settlement', () => {
    it('should calculate correct trading fee (0.3%)', () => {
      const totalValue = 1000000000000000000n; // 1 WRF
      const fee = marketplaceService.calculateFee(totalValue);

      // 0.3% = 30 basis points / 10000 = 0.003
      // 1 WRF * 30 / 10000 = 3000000000000000n (0.003 WRF)
      expect(fee).toBe(3000000000000000n); // 0.3% of 1 WRF
    });

    it('should include fee in trade execution', async () => {
      await marketplaceService.createOrder(
        'seller1',
        OrderType.SELL,
        CreditType.VCS,
        10000000000000000n,
        1000000000000000000n
      );

      await marketplaceService.createOrder(
        'buyer1',
        OrderType.BUY,
        CreditType.VCS,
        10000000000000000n,
        1000000000000000000n
      );

      const trades = await marketplaceService.matchOrders(CreditType.VCS);

      expect(trades[0].fee).toBeGreaterThan(0n);
      expect(trades[0].totalValue).toBe(trades[0].quantity * trades[0].price);
    });

    it('should mark matched orders as filled', async () => {
      const sellOrder = await marketplaceService.createOrder(
        'seller1',
        OrderType.SELL,
        CreditType.VCS,
        10000000000000000n,
        1000000000000000000n
      );

      const buyOrder = await marketplaceService.createOrder(
        'buyer1',
        OrderType.BUY,
        CreditType.VCS,
        10000000000000000n,
        1000000000000000000n
      );

      await marketplaceService.matchOrders(CreditType.VCS);

      const updatedSell = marketplaceService.getOrder(sellOrder.id);
      const updatedBuy = marketplaceService.getOrder(buyOrder.id);

      expect(updatedSell?.status).toBe(OrderStatus.FILLED);
      expect(updatedBuy?.status).toBe(OrderStatus.FILLED);
    });
  });
});
